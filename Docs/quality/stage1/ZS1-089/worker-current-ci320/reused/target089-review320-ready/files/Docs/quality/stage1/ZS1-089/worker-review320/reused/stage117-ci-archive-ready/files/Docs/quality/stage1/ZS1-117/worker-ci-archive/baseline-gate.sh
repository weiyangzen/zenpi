(cd dist && shasum -a 256 -c *.sha256)
! tar -tzf dist/*.tar.gz | grep -Eiq '(auth\.json|\.codex|\.zenpi|fixture|\.env)'
