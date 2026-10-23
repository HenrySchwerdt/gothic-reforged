#!/usr/bin/env bash
# Builds the Quest APK without gradle: cargo (NDK clang) -> aapt2 link -> add .so -> zipalign -> apksigner.
# Usage: scripts/quest/build-apk.sh [release|profiling]    (run from Git Bash)
set -euo pipefail
R=$(cd "$(dirname "$0")/../.." && pwd)
PROFILE=${1:-release}
source "$R/.local/quest-env.sh"
BT="$ANDROID_HOME/build-tools/35.0.0"
PLAT="$ANDROID_HOME/platforms/android-34/android.jar"
OUT="$R/.local/quest/apk"
mkdir -p "$OUT/lib/arm64-v8a"
cargo +stable-x86_64-pc-windows-gnu build -p gothic-quest --lib --target aarch64-linux-android --profile "$PROFILE"
SO="$CARGO_TARGET_DIR/aarch64-linux-android/$PROFILE/libgothic_quest.so"
cp "$SO" "$OUT/lib/arm64-v8a/libgothic_quest.so"
"$NDKBIN/llvm-strip.exe" --strip-unneeded "$OUT/lib/arm64-v8a/libgothic_quest.so"
cp "$R/.local/tools/openxr-loader/prefab/modules/openxr_loader/libs/android.arm64-v8a/libopenxr_loader.so" "$OUT/lib/arm64-v8a/"
cp "$R/.local/tools/ndk/android-ndk-r28c/toolchains/llvm/prebuilt/windows-x86_64/sysroot/usr/lib/aarch64-linux-android/libc++_shared.so" "$OUT/lib/arm64-v8a/"
cd "$OUT"
"$BT/aapt2.exe" link -o base.apk --manifest "$(cygpath -w $R/apps/quest/android/AndroidManifest.xml)" -I "$(cygpath -w $PLAT)" --min-sdk-version 29 --target-sdk-version 32
# add native libs (stored uncompressed + page aligned by zipalign -P 16)
rm -rf stage && mkdir stage && cd stage && unzip -q ../base.apk && mkdir -p lib && cp -r ../lib/arm64-v8a lib/ && cd ..
python -c "
import zipfile,os
z=zipfile.ZipFile('unaligned.apk','w')
for root,_,fs in os.walk('stage'):
    for f in fs:
        p=os.path.join(root,f); n=os.path.relpath(p,'stage').replace(os.sep,'/')
        ct=zipfile.ZIP_STORED if (n.endswith('.so') or n=='resources.arsc') else zipfile.ZIP_DEFLATED
        z.write(p,n,compress_type=ct)
z.close()"
rm -f aligned.apk
"$BT/zipalign.exe" -f -P 16 4 unaligned.apk aligned.apk
KS="$R/.local/quest/debug.keystore"
if [ ! -f "$KS" ]; then
  "/c/Program Files/Java/jdk-18.0.2/bin/keytool.exe" -genkeypair -keystore "$(cygpath -w $KS)" -storepass android -keypass android -alias dev -keyalg RSA -keysize 2048 -validity 10000 -dname "CN=GothicReforged Dev" >/dev/null
fi
"$BT/apksigner.bat" sign --ks "$(cygpath -w $KS)" --ks-pass pass:android --key-pass pass:android --out gothic-quest.apk aligned.apk
ls -l gothic-quest.apk
