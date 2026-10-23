#!/usr/bin/env bash
# Build, install and launch on the Quest, then tail app stats. Usage: scripts/quest/run.sh [release|profiling]
set -euo pipefail
R=$(cd "$(dirname "$0")/../.." && pwd)
D=${QUEST_SERIAL:-2G0YC1ZG1003WN}
PKG=dev.gothicreforged.questtest
bash "$R/scripts/quest/build-apk.sh" "${1:-release}"
npx -y metavr app install -d $D "$(cygpath -w $R/.local/quest/apk/gothic-quest.apk)"
npx -y metavr adb -d $D shell logcat -c
npx -y metavr app launch -d $D --cold-start $PKG
