#!/bin/bash
# Double-click start for macOS: starts Cockpit and opens the browser. Calls scripts/start.sh
# (builds the app the first time; that takes a few minutes).
#
# The first time, macOS may ask whether you want to open the file: right-click the file,
# "Open", "Open" again. To quit, close the window or press Ctrl-C.

cd "$(dirname "$0")" || exit 1

wait_for_key() {
  echo
  read -r -n 1 -s -p "Press any key to close the window ..."
  echo
}

if [ -x scripts/start.sh ]; then
  scripts/start.sh --open
  code=$?
else
  echo "scripts/start.sh is missing here."
  echo "Is this file in the folder of the Cockpit repo?"
  code=1
fi

echo
if [ "$code" -ne 0 ]; then
  echo "Cockpit exited with an error (code $code). Read the messages above or"
  echo "the \"Setup\" section of the README."
else
  echo "Cockpit has exited."
fi
wait_for_key
exit "$code"
