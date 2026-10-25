#!/bin/zsh
set -eu
HERE="${0:A:h}"
STATE="$HOME/Library/Application Support/PPTMergerOffice"
mkdir -p "$STATE"
chmod 700 "$STATE"
if [[ ! -f "$STATE/localhost.key" ]]; then
  /usr/bin/openssl req -x509 -newkey rsa:2048 -nodes -days 90 -keyout "$STATE/localhost.key" -out "$STATE/localhost.crt" -subj '/CN=localhost/O=PPT Merger Local' -addext 'subjectAltName=DNS:localhost,IP:127.0.0.1' -addext 'basicConstraints=critical,CA:FALSE' -addext 'extendedKeyUsage=serverAuth'
  chmod 600 "$STATE/localhost.key"
fi
print 'S’instal·larà el certificat HTTPS local de PPT Merger (localhost), i el manifest del complement amb permís per llegir i modificar la presentació oberta.'
if [[ "${1:-}" != --approved ]]; then
  read 'PPT_APPROVE?Escriu SI per continuar: '
  [[ "$PPT_APPROVE" == SI ]] || exit 1
fi
/usr/bin/security add-trusted-cert -r trustRoot -p ssl -s localhost -k "$HOME/Library/Keychains/login.keychain-db" "$STATE/localhost.crt"
DEST="$HOME/Library/Containers/com.microsoft.Powerpoint/Data/Documents/wef"
if ! mkdir -p "$DEST" || ! cp "$HERE/manifest.xml" "$DEST/PPTMerger-Local.xml"; then
  print 'macOS ha bloquejat la còpia al contenidor de PowerPoint. No cal donar accés general al disc.'
  print "Des del Finder, crea la carpeta: $DEST"
  print "Copia-hi aquest fitxer: $HERE/manifest.xml"
  print 'Després executa Start-Mac.command. No tornis a instal·lar el certificat si ja s’ha completat.'
  exit 1
fi
print 'Manifest instal·lat. Desa el treball obert abans de reiniciar PowerPoint. Després: Inici → Complements → PPT Merger · Format d’origen.'
print 'Executa Start-Mac.command i mantén la finestra oberta mentre utilitzis el complement.'
