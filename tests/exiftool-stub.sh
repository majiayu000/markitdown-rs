#!/bin/sh
# A stand-in for exiftool with fixed output, so the metadata formatting of
# both implementations can be compared without installing exiftool.
if [ "$1" = "-ver" ]; then
  echo "13.10"
  exit 0
fi
cat > /dev/null
cat <<'JSON'
[{"SourceFile": "-", "ImageSize": "4x3", "Title": "Stub title", "Keywords": ["red", "it's", "pixel"],
  "Artist": "A. Artist", "DateTimeOriginal": "2026:10:22 09:30:00", "GPSPosition": "35 deg 41' 22.20\" N",
  "Track": 7, "SampleRate": 44100.5, "NumChannels": 2, "Genre": null, "Album": true,
  "Description": {"lang": "en", "text": "nested"}}]
JSON
