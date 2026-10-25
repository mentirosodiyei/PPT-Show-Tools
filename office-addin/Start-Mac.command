#!/bin/zsh
set -eu
HERE="${0:A:h}"
STATE="$HOME/Library/Application Support/PPTMergerOffice"
if [[ -x "$HERE/../../MacOS/PPTMerger" ]]; then
  PPT_BIN="$HERE/../../MacOS/PPTMerger"
else
  PPT_BIN="$HERE/../dist/PPT Merger.app/Contents/MacOS/PPTMerger"
fi
if [[ ! -f "$STATE/localhost.crt" || ! -f "$STATE/localhost.key" ]]; then
  print 'Cal executar Setup-Mac.command abans de començar.'
  read 'PPT_WAIT?Prem Retorn per tancar. '
  exit 1
fi
exec /usr/bin/python3 "$HERE/server.py" --bin "$PPT_BIN" --cert "$STATE/localhost.crt" --key "$STATE/localhost.key" --work "$STATE/jobs"
