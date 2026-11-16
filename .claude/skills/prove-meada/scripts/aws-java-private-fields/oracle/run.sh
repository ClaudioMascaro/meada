#!/bin/sh
# The answer key of the probe (#775): each case compiled with the stub SDK and run, on a JDK.
# Usage: sh run.sh > java-run.txt (from this folder). Prints `java -version` and each case's sends.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
java -version 2>&1 | head -n 1
for case in a b c; do
  mkdir "$work/$case"
  javac -d "$work/$case" $(find "$here/sdk" -name '*.java') "$here/../$case"/*.java "$here/$case/Main.java"
  echo "case $case:"
  java -cp "$work/$case" Main
done
