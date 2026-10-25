#!/usr/bin/env python3
"""PPT Merger: structural OOXML loss check. No Office or third-party packages.

Usage:
    python3 pptx_audit.py --inputs A.pptx B.pptx --merged fusion.pptx --json audit.json

This is a diagnostics tool, NOT a PowerPoint merger and NOT a visual-fidelity proof.
"""
from __future__ import annotations
import argparse
from collections import Counter
import hashlib
import json
import posixpath
from pathlib import Path
import re
import sys
import zipfile
from urllib.parse import unquote, urlsplit
import xml.etree.ElementTree as ET

P = "http://schemas.openxmlformats.org/presentationml/2006/main"
R = "http://schemas.openxmlformats.org/officeDocument/2006/relationships"
PKG_R = "http://schemas.openxmlformats.org/package/2006/relationships"
NS = {"p": P, "r": R}


def relation_owner(rel_path):
    if rel_path == "_rels/.rels":
        return ""
    match = re.fullmatch(r"(?:(.+)/)?_rels/([^/]+)\.rels", rel_path)
    if not match:
        return None
    directory, basename = match.groups()
    return posixpath.join(directory or "", basename)


def target_path(owner, target):
    # Only internal OPC relationships are passed here. Unescape percent-encoding.
    u = urlsplit(target)
    path = unquote(u.path)
    if path.startswith("/"):
        return posixpath.normpath(path.lstrip("/"))
    return posixpath.normpath(posixpath.join(posixpath.dirname(owner), path))


def digest_stream(fileobj):
    h = hashlib.sha256()
    while True:
        chunk = fileobj.read(1024 * 1024)
        if not chunk:
            return h.hexdigest()
        h.update(chunk)


def xml_fingerprint(element):
    if element is None:
        return None
    # Ignore prefix spelling and attribute order, preserve structure and values.
    def tree(e):
        return [e.tag, sorted(e.attrib.items()), e.text or '', [tree(c) for c in e]]
    return hashlib.sha256(json.dumps(tree(element), ensure_ascii=False).encode()).hexdigest()


def inspect_deck(path):
    deck = {"path": str(path), "slide_count": 0, "master_count": 0,
            "layout_count": 0, "theme_count": 0, "notes_count": 0,
            "custom_show_count": 0, "transitions": 0, "timings": 0,
            "slide_hyperlink_relations": 0, "slide_chart_relations": 0,
            "slide_ole_relations": 0, "slide_video_relations": 0,
            "slide_audio_relations": 0, "slide_action_relations": 0,
            "slide_internal_links": 0, "binary_hashes": {},
            "missing_targets": [], "invalid_slide_chain": [], "warnings": [], "fatal_errors": [], "effective_notes_masters": [], "slide_features": [], "active_font_hashes": []}
    try:
        with zipfile.ZipFile(path, 'r') as z:
            names = set(z.namelist())
            if len(names) != len(z.namelist()):
                deck['warnings'].append("Duplicate ZIP entries present")
            for required in ('[Content_Types].xml', '_rels/.rels',
                             'ppt/presentation.xml', 'ppt/_rels/presentation.xml.rels'):
                if required not in names:
                    deck['fatal_errors'].append(f"Missing package part: {required}")
            if 'ppt/presentation.xml' not in names:
                return deck
            relations = {}
            for rp in sorted(x for x in names if x.endswith('.rels')):
                owner = relation_owner(rp)
                if owner is None:
                    deck['warnings'].append(f"Unrecognized relations path: {rp}")
                    continue
                try:
                    root = ET.fromstring(z.read(rp))
                except ET.ParseError as ex:
                    deck['warnings'].append(f"Invalid XML in {rp}: {ex}")
                    continue
                these = {}
                for rel in root.findall(f'{{{PKG_R}}}Relationship'):
                    rid = rel.get('Id', '')
                    typ = rel.get('Type', '')
                    target = rel.get('Target', '')
                    if rel.get('TargetMode') == 'External':
                        these[rid] = ('EXTERNAL', typ)
                        continue
                    if not target:
                        deck['missing_targets'].append({'from': owner, 'id': rid, 'target': target})
                        continue
                    resolved = target_path(owner, target)
                    these[rid] = (resolved, typ)
                    if resolved not in names:
                        deck['missing_targets'].append({'from': owner, 'id': rid, 'target': resolved})
                relations[owner] = these
            pres = ET.fromstring(z.read('ppt/presentation.xml'))
            deck['custom_show_count'] = len(pres.findall('.//p:custShow', NS))
            slide_ids = pres.findall('./p:sldIdLst/p:sldId', NS)
            pres_rels = relations.get('ppt/presentation.xml', {})
            for font in pres.findall('./p:embeddedFontLst/p:embeddedFont', NS):
                for style in font:
                    rid = style.get(f'{{{R}}}id')
                    if not rid:
                        continue
                    entry = pres_rels.get(rid)
                    if not entry or entry[0] not in names:
                        deck['fatal_errors'].append(f'Broken embedded font registration: {rid}')
                    else:
                        with z.open(entry[0]) as fp:
                            deck['active_font_hashes'].append(digest_stream(fp))
            slides = []
            for si, sid in enumerate(slide_ids, start=1):
                rid = sid.get(f'{{{R}}}id', '')
                entry = pres_rels.get(rid)
                if not entry or entry[0] in ('EXTERNAL', '') or entry[0] not in names:
                    deck['invalid_slide_chain'].append({'slide': si, 'problem': 'missing presentation->slide relation', 'rid': rid})
                    continue
                slides.append(entry[0])
            deck['slide_count'] = len(slide_ids)
            # OPC permits parts outside conventional ppt/* folders. Use declared
            # content types, not filename conventions, to count relocated parts.
            content_types = ET.fromstring(z.read('[Content_Types].xml'))
            overrides = {unquote(e.get('PartName', '').lstrip('/')): e.get('ContentType', '')
                         for e in content_types if e.tag.endswith('}Override')}
            defaults = {e.get('Extension', '').lower(): e.get('ContentType', '')
                        for e in content_types if e.tag.endswith('}Default')}
            def content_type(name):
                return overrides.get(name, defaults.get(name.rsplit('.', 1)[-1].lower(), ''))
            for metric, suffix in [('master_count', 'presentationml.slideMaster+xml'),
                                   ('layout_count', 'presentationml.slideLayout+xml'),
                                   ('theme_count', 'officedocument.theme+xml'),
                                   ('notes_count', 'presentationml.notesSlide+xml')]:
                deck[metric] = sum(content_type(n).endswith(suffix) for n in names)
            for idx, sp in enumerate(slides, start=1):
                try:
                    sr = ET.fromstring(z.read(sp))
                    deck['transitions'] += int(sr.find(f'{{{P}}}transition') is not None)
                    deck['timings'] += int(sr.find(f'{{{P}}}timing') is not None)
                    deck['slide_features'].append({
                        'timing': xml_fingerprint(sr.find(f'{{{P}}}timing')),
                        'transition': xml_fingerprint(sr.find(f'{{{P}}}transition')),
                        'text': [e.text or '' for e in sr.iter()
                                 if e.tag == '{http://schemas.openxmlformats.org/drawingml/2006/main}t']})
                except ET.ParseError as ex:
                    deck['warnings'].append(f'Invalid slide XML {sp}: {ex}')
                slide_rel = relations.get(sp, {})
                note_parts = [dest for dest, typ in slide_rel.values() if typ.endswith('/notesSlide')]
                note_master_fingerprints = []
                for note_part in note_parts:
                    for master_part, typ in relations.get(note_part, {}).values():
                        if typ.endswith('/notesMaster') and master_part in names:
                            master_xml = ET.fromstring(z.read(master_part))
                            themes = sorted(xml_fingerprint(ET.fromstring(z.read(dest)))
                                            for dest, rel_type in relations.get(master_part, {}).values()
                                            if rel_type.endswith('/theme') and dest in names)
                            note_master_fingerprints.append([xml_fingerprint(master_xml), themes])
                deck['effective_notes_masters'].append(sorted(note_master_fingerprints))

                layout = [target for target, typ in slide_rel.values() if typ.endswith('/slideLayout')]
                if len(layout) != 1 or layout[0] not in names:
                    deck['invalid_slide_chain'].append({'slide': idx, 'problem': 'slide->layout', 'parts': layout})
                else:
                    master = [target for target, typ in relations.get(layout[0], {}).values() if typ.endswith('/slideMaster')]
                    if len(master) != 1 or master[0] not in names:
                        deck['invalid_slide_chain'].append({'slide': idx, 'problem': 'layout->master', 'parts': master})
                for dest, typ in slide_rel.values():
                    short = typ.rsplit('/', 1)[-1].lower()
                    if short == 'hyperlink':
                        deck['slide_hyperlink_relations'] += 1
                    elif short == 'chart':
                        deck['slide_chart_relations'] += 1
                    elif short in ('oleobject', 'package'):
                        deck['slide_ole_relations'] += 1
                    elif short == 'video':
                        deck['slide_video_relations'] += 1
                    elif short == 'audio':
                        deck['slide_audio_relations'] += 1
                    elif short == 'slide':
                        deck['slide_internal_links'] += 1
                # Action buttons often use hlinks; that control is structural only.
            for name in sorted(names):
                if not name.endswith('/') and any(segment in name.split('/')[:-1] for segment in ('media', 'embeddings', 'fonts')):
                    with z.open(name) as fp:
                        digest = digest_stream(fp)
                    deck['binary_hashes'].setdefault(digest, []).append(name)
    except (OSError, zipfile.BadZipFile, ET.ParseError) as ex:
        deck['fatal_errors'].append(f'Unable to read presentation: {type(ex).__name__}: {ex}')
    return deck


METRIC_NAMES = [
    'slide_count', 'notes_count', 'transitions', 'timings',
    'slide_hyperlink_relations', 'slide_chart_relations', 'slide_ole_relations',
    'slide_video_relations', 'slide_audio_relations', 'slide_internal_links',
    'custom_show_count'
]


def compare(inputs, result):
    errors, warnings = [], []
    for deck in inputs + [result]:
        errors.extend(f"{deck['path']}: {issue}" for issue in deck.get('fatal_errors', []))
    for deck in inputs:
        if deck['missing_targets'] or deck['invalid_slide_chain']:
            errors.append(f"{deck['path']}: invalid source relationships or slide chains")
        warnings.extend(f"Source {deck['path']}: {issue}" for issue in deck['warnings'])
    expected_slides = sum(d['slide_count'] for d in inputs)
    if expected_slides != result['slide_count']:
        errors.append(f"Slide count {result['slide_count']} instead of {expected_slides}")
    expected_features = [x for d in inputs for x in d.get('slide_features', [])]
    actual_features = result.get('slide_features', [])
    for feature in ('timing', 'transition', 'text'):
        changed = [i + 1 for i, (before, after) in enumerate(zip(expected_features, actual_features))
                   if before[feature] != after[feature]]
        if changed:
            warnings.append(f'Per-slide {feature} differs on {len(changed)} slides (first: {changed[:10]}). Review required; XML rewrites or text run splitting can cause false positives.')
    expected_notes = [x for d in inputs for x in d['effective_notes_masters']]
    actual_notes = result['effective_notes_masters']
    changed_notes = [i + 1 for i, (before, after) in enumerate(zip(expected_notes, actual_notes)) if before != after]
    if changed_notes:
        warnings.append(f"Effective notes master differs on {len(changed_notes)} slides (first: {changed_notes[:10]}). Inspect notes formatting; XML changes may also be harmless rewrites.")
    missing_fonts = {h for d in inputs for h in d.get('active_font_hashes', [])} - set(result.get('active_font_hashes', []))
    if missing_fonts:
        errors.append(f'{len(missing_fonts)} active embedded font payloads no longer registered in merged presentation')
    original_hashes = {h for d in inputs for h in d['binary_hashes']}
    missing = original_hashes - set(result['binary_hashes'])
    if missing:
        errors.append(f'{len(missing)} unique binary payloads missing in merged PPTX')
    if result['missing_targets']:
        errors.append(f"{len(result['missing_targets'])} unresolved internal relationships")
    if result['invalid_slide_chain']:
        errors.append(f"{len(result['invalid_slide_chain'])} invalid slide/layout/master chains")
    if result['warnings']:
        warnings.extend(result['warnings'])
    for name in METRIC_NAMES:
        if name == 'slide_count':
            continue
        source_total = sum(d[name] for d in inputs)
        merged_total = result[name]
        if merged_total < source_total:
            warnings.append(f'{name}: source total {source_total}, merged {merged_total} (possible loss)')
    for name in ('master_count', 'layout_count', 'theme_count'):
        source_total = sum(d[name] for d in inputs)
        merged_total = result[name]
        if merged_total < source_total:
            warnings.append(f'{name}: source total {source_total}, merged {merged_total} (verify intentional deduplication)')
    return {'structural_gate': 'FAIL' if errors else ('REVIEW' if warnings else 'PASS'),
            'errors': errors, 'warnings': warnings,
            'missing_binary_hashes': sorted(missing),
            'note': 'PASS only means these structural checks passed; open PowerPoint for visual, playback, action and timing QA.'}


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('--inputs', nargs='+', type=Path, required=True, metavar='PPTX')
    ap.add_argument('--merged', type=Path, required=True)
    ap.add_argument('--json', type=Path, metavar='REPORT')
    args = ap.parse_args()
    inputs = [inspect_deck(f) for f in args.inputs]
    merged = inspect_deck(args.merged)
    quality = compare(inputs, merged)
    for deck in inputs + [merged]:
        # Keep hashes in report, but avoid duplicated file path listings in summary.
        deck['binary_unique_count'] = len(deck.pop('binary_hashes'))
    report = {'sources': inputs, 'merged': merged, 'assessment': quality}
    if args.json:
        args.json.parent.mkdir(parents=True, exist_ok=True)
        args.json.write_text(json.dumps(report, indent=2, ensure_ascii=False), encoding='utf-8')
    print('STRUCTURAL GATE:', quality['structural_gate'])
    print('SOURCE SLIDES:', sum(d['slide_count'] for d in inputs), '| MERGED SLIDES:', merged['slide_count'])
    for key, data in [('ERROR', quality['errors']), ('WARNING', quality['warnings'])]:
        for item in data:
            print(key + ':', item)
    if args.json:
        print('Report:', args.json)
    return 2 if quality['errors'] else (1 if quality['warnings'] else 0)


if __name__ == '__main__':
    sys.exit(main())
