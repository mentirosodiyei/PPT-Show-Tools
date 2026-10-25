//! Minimal XML DOM that round-trips Open XML parts: element and attribute
//! order, prefixes, namespace declarations and whitespace text are preserved.
use crate::{fail, Result};
use quick_xml::events::Event;
use quick_xml::Reader;
use std::path::Path;

pub const P_NS: &str = "http://schemas.openxmlformats.org/presentationml/2006/main";
pub const R_NS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
pub const PKG_NS: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
pub const A_NS: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
pub const CT_NS: &str = "http://schemas.openxmlformats.org/package/2006/content-types";
const XMLNS_NS: &str = "http://www.w3.org/2000/xmlns/";
const XML_NS: &str = "http://www.w3.org/XML/1998/namespace";

#[derive(Clone, Debug)]
pub struct Attr {
    pub qname: String,
    pub ns: String,
    pub value: String,
}
impl Attr {
    pub fn local(&self) -> &str {
        self.qname.rsplit(':').next().unwrap_or("")
    }
    fn is_declaration(&self) -> bool {
        self.ns == XMLNS_NS
    }
}

#[derive(Clone, Debug)]
pub enum Node {
    Elem(Element),
    Text(String),
    CData(String),
    Comment(String),
    PI(String),
}

#[derive(Clone, Debug)]
pub struct Element {
    pub qname: String,
    pub ns: String,
    pub attrs: Vec<Attr>,
    pub children: Vec<Node>,
}

impl Element {
    pub fn new(qname: &str, ns: &str) -> Element {
        Element { qname: qname.into(), ns: ns.into(), attrs: vec![], children: vec![] }
    }
    pub fn local(&self) -> &str {
        self.qname.rsplit(':').next().unwrap_or("")
    }
    /// Attribute by qualified name, "" when absent (like the Swift `attr`).
    pub fn attr(&self, qname: &str) -> &str {
        self.attrs.iter().find(|a| a.qname == qname).map(|a| a.value.as_str()).unwrap_or("")
    }
    pub fn has_attr(&self, qname: &str) -> bool {
        self.attrs.iter().any(|a| a.qname == qname)
    }
    pub fn attr_mut(&mut self, qname: &str) -> Option<&mut String> {
        self.attrs.iter_mut().find(|a| a.qname == qname).map(|a| &mut a.value)
    }
    pub fn set_attr(&mut self, qname: &str, value: &str) {
        match self.attrs.iter_mut().find(|a| a.qname == qname) {
            Some(a) => a.value = value.into(),
            None => self.attrs.push(Attr { qname: qname.into(), ns: String::new(), value: value.into() }),
        }
    }
    pub fn remove_attr(&mut self, qname: &str) {
        self.attrs.retain(|a| a.qname != qname);
    }
    pub fn attr_ns(&self, ns: &str, local: &str) -> &str {
        self.attrs.iter().find(|a| a.ns == ns && a.local() == local).map(|a| a.value.as_str()).unwrap_or("")
    }
    pub fn set_attr_ns(&mut self, ns: &str, local: &str, prefix: &str, value: &str) {
        match self.attrs.iter_mut().find(|a| a.ns == ns && a.local() == local) {
            Some(a) => a.value = value.into(),
            None => self.attrs.push(Attr { qname: format!("{prefix}:{local}"), ns: ns.into(), value: value.into() }),
        }
    }
    /// r:id of a presentation-level element.
    pub fn rid(&self) -> &str {
        self.attr_ns(R_NS, "id")
    }
    pub fn elems(&self) -> impl Iterator<Item = &Element> {
        self.children.iter().filter_map(|n| if let Node::Elem(e) = n { Some(e) } else { None })
    }
    pub fn elems_mut(&mut self) -> impl Iterator<Item = &mut Element> {
        self.children.iter_mut().filter_map(|n| if let Node::Elem(e) = n { Some(e) } else { None })
    }
    pub fn named<'a>(&'a self, local: &'a str) -> impl Iterator<Item = &'a Element> + 'a {
        self.elems().filter(move |e| e.local() == local)
    }
    pub fn child(&self, local: &str) -> Option<&Element> {
        self.elems().find(|e| e.local() == local)
    }
    pub fn child_mut(&mut self, local: &str) -> Option<&mut Element> {
        self.elems_mut().find(|e| e.local() == local)
    }
    pub fn add(&mut self, e: Element) {
        self.children.push(Node::Elem(e));
    }
    /// Concatenated descendant text (Foundation's `stringValue`).
    pub fn text(&self) -> String {
        let mut s = String::new();
        fn go(e: &Element, s: &mut String) {
            for c in &e.children {
                match c {
                    Node::Elem(x) => go(x, s),
                    Node::Text(t) | Node::CData(t) => s.push_str(t),
                    _ => {}
                }
            }
        }
        go(self, &mut s);
        s
    }
    /// Pre-order visit of this element and every descendant element.
    pub fn visit<'a>(&'a self, f: &mut dyn FnMut(&'a Element)) {
        f(self);
        for e in self.elems() {
            e.visit(f);
        }
    }
    pub fn visit_mut(&mut self, f: &mut dyn FnMut(&mut Element)) {
        f(self);
        for e in self.elems_mut() {
            e.visit_mut(f);
        }
    }
    /// Descendants (excluding self) with the given local name: `.//*[local-name()=…]`.
    pub fn descendants<'a>(&'a self, local: &str) -> Vec<&'a Element> {
        let mut out = vec![];
        for c in self.elems() {
            c.visit(&mut |e| if e.local() == local { out.push(e) });
        }
        out
    }
    pub fn first_descendant_mut(&mut self, local: &str) -> Option<&mut Element> {
        for c in self.elems_mut() {
            if c.local() == local {
                return Some(c);
            }
            if let Some(x) = c.first_descendant_mut(local) {
                return Some(x);
            }
        }
        None
    }
}

pub struct Document {
    pub root: Element,
    prolog: Vec<Node>,
}

fn resolve(scopes: &[Vec<(String, String)>], prefix: &str) -> Option<String> {
    if prefix == "xml" {
        return Some(XML_NS.into());
    }
    if prefix == "xmlns" {
        return Some(XMLNS_NS.into());
    }
    for scope in scopes.iter().rev() {
        if let Some((_, uri)) = scope.iter().rev().find(|(p, _)| p == prefix) {
            return Some(uri.clone());
        }
    }
    None
}

fn utf8(bytes: &[u8]) -> Result<String> {
    String::from_utf8(bytes.to_vec()).or_else(|_| fail("XML amb codificació no admesa (cal UTF-8)."))
}

impl Document {
    pub fn parse(data: &[u8]) -> Result<Document> {
        let data = data.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(data);
        if data.starts_with(b"\xFF\xFE") || data.starts_with(b"\xFE\xFF") {
            return fail("XML en UTF-16 no admès.");
        }
        let text = std::str::from_utf8(data).or_else(|_| fail("XML amb codificació no admesa (cal UTF-8)."))?;
        let mut reader = Reader::from_str(text);
        reader.config_mut().trim_text(false);
        reader.config_mut().check_end_names = true;
        let mut stack: Vec<Element> = vec![];
        let mut scopes: Vec<Vec<(String, String)>> = vec![];
        let mut prolog = vec![];
        let mut root: Option<Element> = None;
        loop {
            let event = reader.read_event().or_else(|e| fail(format!("XML no vàlid: {e}")))?;
            let push = |node: Node, stack: &mut Vec<Element>, prolog: &mut Vec<Node>| match stack.last_mut() {
                Some(parent) => parent.children.push(node),
                None => {
                    if !matches!(node, Node::Text(_)) {
                        prolog.push(node)
                    }
                }
            };
            match event {
                Event::Start(ref e) | Event::Empty(ref e) => {
                    let empty = matches!(event, Event::Empty(_));
                    let qname = utf8(e.name().as_ref())?;
                    let mut raw = vec![];
                    let mut scope = vec![];
                    for a in e.attributes() {
                        let a = a.or_else(|err| fail(format!("Atribut XML no vàlid: {err}")))?;
                        let key = utf8(a.key.as_ref())?;
                        // Attribute-value normalization: literal tabs and line breaks are spaces.
                        let rawv: String = utf8(&a.value)?.chars().map(|c| if c == '\t' || c == '\n' || c == '\r' { ' ' } else { c }).collect();
                        let value = quick_xml::escape::unescape(&rawv).or_else(|err| fail(format!("Entitat XML no vàlida: {err}")))?.into_owned();
                        if key == "xmlns" {
                            scope.push((String::new(), value.clone()));
                        } else if let Some(p) = key.strip_prefix("xmlns:") {
                            scope.push((p.to_string(), value.clone()));
                        }
                        raw.push((key, value));
                    }
                    scopes.push(scope);
                    let (prefix, _) = qname.rsplit_once(':').unwrap_or(("", &qname));
                    let ns = resolve(&scopes, prefix).unwrap_or_default();
                    let mut attrs = vec![];
                    for (key, value) in raw {
                        let ans = if key == "xmlns" || key.starts_with("xmlns:") {
                            XMLNS_NS.to_string()
                        } else if let Some((p, _)) = key.split_once(':') {
                            resolve(&scopes, p).unwrap_or_default()
                        } else {
                            String::new()
                        };
                        attrs.push(Attr { qname: key, ns: ans, value });
                    }
                    let element = Element { qname, ns, attrs, children: vec![] };
                    if empty {
                        scopes.pop();
                        match stack.last_mut() {
                            Some(parent) => parent.children.push(Node::Elem(element)),
                            None => root = Some(element),
                        }
                    } else {
                        stack.push(element);
                    }
                }
                Event::End(_) => {
                    scopes.pop();
                    let element = stack.pop().ok_or_else(|| crate::Error::Fail("XML no vàlid.".into()))?;
                    match stack.last_mut() {
                        Some(parent) => parent.children.push(Node::Elem(element)),
                        None => root = Some(element),
                    }
                }
                Event::Text(t) => {
                    let s = t.unescape().or_else(|err| fail(format!("Text XML no vàlid: {err}")))?.into_owned();
                    push(Node::Text(s), &mut stack, &mut prolog);
                }
                Event::CData(t) => push(Node::CData(utf8(&t.into_inner())?), &mut stack, &mut prolog),
                Event::Comment(t) => push(Node::Comment(utf8(&t.into_inner())?), &mut stack, &mut prolog),
                Event::PI(t) => push(Node::PI(utf8(&t)?), &mut stack, &mut prolog),
                Event::Decl(_) | Event::DocType(_) => {}
                Event::Eof => break,
            }
        }
        if !stack.is_empty() {
            return fail("XML incomplet.");
        }
        let root = root.ok_or_else(|| crate::Error::Fail("XML sense element arrel.".into()))?;
        Ok(Document { root, prolog })
    }

    /// Prefix bound to `ns` on the root element, declaring `fallback` if needed.
    pub fn prefix_for(&mut self, ns: &str, fallback: &str) -> String {
        if let Some(a) = self.root.attrs.iter().find(|a| a.is_declaration() && a.value == ns && a.qname.starts_with("xmlns:")) {
            return a.local().to_string();
        }
        self.root.attrs.push(Attr { qname: format!("xmlns:{fallback}"), ns: XMLNS_NS.into(), value: ns.into() });
        fallback.to_string()
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut s = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n");
        for n in &self.prolog {
            write_node(n, &mut s);
        }
        write_element(&self.root, &mut s);
        s.into_bytes()
    }
}

fn escape_text(t: &str, out: &mut String) {
    for c in t.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '\r' => out.push_str("&#13;"),
            _ => out.push(c),
        }
    }
}
// Whitespace inside attributes must be character references, or parsers
// normalize it to spaces and alt text loses its line breaks.
fn escape_attr(t: &str, out: &mut String) {
    for c in t.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\n' => out.push_str("&#10;"),
            '\r' => out.push_str("&#13;"),
            '\t' => out.push_str("&#9;"),
            _ => out.push(c),
        }
    }
}
fn write_node(n: &Node, out: &mut String) {
    match n {
        Node::Elem(e) => write_element(e, out),
        Node::Text(t) => escape_text(t, out),
        Node::CData(t) => {
            out.push_str("<![CDATA[");
            out.push_str(t);
            out.push_str("]]>");
        }
        Node::Comment(t) => {
            out.push_str("<!--");
            out.push_str(t);
            out.push_str("-->");
        }
        Node::PI(t) => {
            out.push_str("<?");
            out.push_str(t);
            out.push_str("?>");
        }
    }
}
fn write_element(e: &Element, out: &mut String) {
    out.push('<');
    out.push_str(&e.qname);
    for a in &e.attrs {
        out.push(' ');
        out.push_str(&a.qname);
        out.push_str("=\"");
        escape_attr(&a.value, out);
        out.push('"');
    }
    if e.children.is_empty() {
        out.push_str("/>");
        return;
    }
    out.push('>');
    for c in &e.children {
        write_node(c, out);
    }
    out.push_str("</");
    out.push_str(&e.qname);
    out.push('>');
}

pub fn part_path(root: &Path, part: &str) -> std::path::PathBuf {
    let mut p = root.to_path_buf();
    for seg in part.split('/').filter(|s| !s.is_empty()) {
        p.push(seg);
    }
    p
}
pub fn read_xml(root: &Path, part: &str) -> Result<Document> {
    let data = std::fs::read(part_path(root, part)).or_else(|e| fail(format!("No es pot llegir {part}: {e}")))?;
    Document::parse(&data).or_else(|e| fail(format!("{part}: {e}")))
}
pub fn write_xml(doc: &Document, root: &Path, part: &str) -> Result<()> {
    crate::package::write_atomic(&part_path(root, part), &doc.to_bytes())
}

/// Order-independent structural fingerprint used to compare parts.
pub fn semantic(e: Option<&Element>) -> String {
    let Some(e) = e else { return "ABSENT".into() };
    fn field(s: &str) -> String {
        format!("{}:{}", s.len(), s)
    }
    fn node(n: &Node, parent_local: &str) -> Option<String> {
        match n {
            Node::Elem(x) => Some(semantic(Some(x))),
            Node::Text(t) | Node::CData(t) | Node::Comment(t) | Node::PI(t) => {
                if !t.trim().is_empty() || parent_local == "t" {
                    Some(format!("T{}", field(t)))
                } else {
                    None
                }
            }
        }
    }
    let mut attrs: Vec<String> = e.attrs.iter().filter(|a| !a.is_declaration()).map(|a| field(&format!("{}|{}", a.ns, a.local())) + &field(&a.value)).collect();
    attrs.sort();
    let body: String = e.children.iter().filter_map(|n| node(n, e.local())).collect();
    format!("E{}{}{}{}", field(&e.ns), field(e.local()), field(&attrs.concat()), field(&body))
}
