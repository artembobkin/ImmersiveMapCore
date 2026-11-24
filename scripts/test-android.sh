#!/usr/bin/env bash
# Runs the workspace's tests on an Android device: builds them for
# aarch64-linux-android with the NDK's clang, pushes them and the fixtures
# to the device adb sees, runs each one there with FIXTURES naming the
# pushed fixtures, and fails if a test fails or a fixture test skips.
#
#   scripts/test-android.sh
#
# Needs the aarch64-linux-android Rust target, the NDK (ANDROID_NDK_HOME,
# else the newest in $ANDROID_HOME/ndk), one device in `adb devices`, and
# the fixtures at the root of the workspace (ImmersiveMap Web's
# `npm run fixtures`, or a symbolic link to its `fixtures`).
set -euo pipefail

cd "$(dirname "$0")/.."
api=29
remote=/data/local/tmp/immersivemap-core-tests

ndk="${ANDROID_NDK_HOME:-}"
if [ -z "$ndk" ] && [ -n "${ANDROID_HOME:-}" ] && [ -d "$ANDROID_HOME/ndk" ]; then
  ndk="$ANDROID_HOME/ndk/$(ls "$ANDROID_HOME/ndk" | sort -V | tail -1)"
fi
[ -d "$ndk" ] || { echo "No NDK: set ANDROID_NDK_HOME." >&2; exit 1; }
# The NDK's prebuilt toolchain is darwin-x86_64 on every Mac.
host="$(uname -s | tr '[:upper:]' '[:lower:]')-x86_64"
clang="$ndk/toolchains/llvm/prebuilt/$host/bin/aarch64-linux-android$api-clang"
[ -x "$clang" ] || { echo "No $clang." >&2; exit 1; }
export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="$clang"
export CC_aarch64_linux_android="$clang"
export AR_aarch64_linux_android="$ndk/toolchains/llvm/prebuilt/$host/bin/llvm-ar"

adb get-state >/dev/null || { echo "No device: check 'adb devices'." >&2; exit 1; }

# The test executables, from cargo's own report of what it built.
tests=()
while IFS= read -r executable; do
  tests+=("$executable")
done < <(
  cargo test --workspace --no-run --target aarch64-linux-android --message-format=json 2>/dev/null |
    grep '"reason":"compiler-artifact"' | grep '"test":true' |
    sed -n 's/.*"executable":"\([^"]*\)".*/\1/p'
)
[ "${#tests[@]}" -gt 0 ] || { echo "No test was built." >&2; exit 1; }
echo "${#tests[@]} test binaries"

adb shell "rm -rf $remote && mkdir -p $remote/fixtures"
adb push "${tests[@]}" "$remote/" >/dev/null
fixtures=0
for directory in tiles models terrain; do
  if [ -d "fixtures/$directory" ]; then
    adb push "fixtures/$directory" "$remote/fixtures/" >/dev/null
    fixtures=1
  fi
done
[ "$fixtures" -eq 1 ] || echo "No fixtures here: the real-tile tests will skip." >&2

passed=0
failed=()
skipped=()
for test in "${tests[@]}"; do
  name=$(basename "$test")
  output=$(adb shell "cd $remote && FIXTURES=$remote/fixtures ./$name 2>&1; echo EXIT=\$?")
  summary=$(echo "$output" | grep '^test result' || true)
  echo "$name: ${summary:-no result}"
  if echo "$output" | grep -q 'EXIT=0'; then
    passed=$((passed + $(echo "$summary" | sed -n 's/.* \([0-9][0-9]*\) passed.*/\1/p')))
  else
    failed+=("$name")
    echo "$output" | grep -E 'FAILED|panicked' | head -5
  fi
  if [ "$fixtures" -eq 1 ] && echo "$output" | grep -qE 'is missing: run|no fixtures at'; then
    skipped+=("$name")
  fi
done

echo
echo "$passed tests passed on $(adb shell getprop ro.product.model | tr -d '\r') (Android $(adb shell getprop ro.build.version.release | tr -d '\r'))."
if [ "${#skipped[@]}" -gt 0 ]; then
  echo "Fixture tests skipped although the fixtures were pushed: ${skipped[*]}" >&2
fi
if [ "${#failed[@]}" -gt 0 ]; then
  echo "Failed: ${failed[*]}" >&2
fi
[ "${#failed[@]}" -eq 0 ] && [ "${#skipped[@]}" -eq 0 ]
