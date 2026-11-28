#!/usr/bin/env python3
"""Build an ARM64 Android APK using the existing engine, SDK/NDK and Gradle.

No game data is read or packaged. Use --check to check Rust without linking an APK.
"""
from __future__ import annotations
import argparse
import os
from pathlib import Path
import platform
import shutil
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[2]
TARGET = "aarch64-linux-android"
API = 29

CLASSIC_CASING = Path("dialogue/classic.png")


def stage_assets(destination: Path) -> None:
    """Stage only tracked, project-owned runtime data and Classic casing artwork."""
    for name in ("schemas", "lang"):
        source = ROOT / name
        for path in source.rglob("*"):
            if path.is_file():
                relative = path.relative_to(source)
                # The allow-list is deliberate: never package arbitrary local files.
                if path.suffix not in {".json", ".yaml", ".yml", ".toml"}:
                    continue
                tracked = subprocess.run(["git", "ls-files", "--error-unmatch", "--", str(path.relative_to(ROOT))],
                                         cwd=ROOT, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
                if tracked.returncode:
                    continue
                output = destination / name / relative
                output.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(path, output)

    casing = Path("engine/nvf-play/assets") / CLASSIC_CASING
    subprocess.run(["git", "ls-files", "--error-unmatch", "--", casing.as_posix()],
                   cwd=ROOT, stdout=subprocess.DEVNULL, check=True)
    output = destination / CLASSIC_CASING
    output.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(ROOT / casing, output)


def build_environment(ndk: Path, target_dir: Path, jobs: int) -> dict[str, str]:
    host = {"Windows": "windows-x86_64", "Linux": "linux-x86_64", "Darwin": "darwin-x86_64"}[platform.system()]
    binary = ndk / "toolchains/llvm/prebuilt" / host / "bin"
    suffix = ".cmd" if platform.system() == "Windows" else ""
    executable = ".exe" if platform.system() == "Windows" else ""
    cc = binary / f"{TARGET}{API}-clang{suffix}"
    if not cc.is_file():
        raise SystemExit(f"Android NDK compiler not found: {cc}")
    env = os.environ.copy()
    env.update({"CARGO_TARGET_DIR": str(target_dir.resolve()), "CARGO_BUILD_JOBS": str(jobs),
                "CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER": str(cc),
                "CC_aarch64_linux_android": str(cc),
                "CXX_aarch64_linux_android": str(binary / f"{TARGET}{API}-clang++{suffix}"),
                "AR_aarch64_linux_android": str(binary / f"llvm-ar{executable}"),
                "ANDROID_NDK_HOME": str(ndk.resolve())})
    return env


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--ndk", type=Path, default=os.environ.get("ANDROID_NDK_HOME"))
    parser.add_argument("--target-dir", type=Path, default=ROOT / "engine/target-android")
    parser.add_argument("--jobs", type=int, default=2)
    parser.add_argument("--gradle", default="gradle")
    parser.add_argument("--check", action="store_true")
    parser.add_argument("--tests", action="store_true", help="Cross-check authored engine tests (does not execute them)")
    parser.add_argument("--gradle-build-dir", type=Path, help="Optional separate Gradle output/cache directory")
    args = parser.parse_args()
    if args.ndk is None or args.jobs < 1:
        parser.error("set ANDROID_NDK_HOME or --ndk, and a positive --jobs")
    args.target_dir = args.target_dir.resolve()
    env = build_environment(args.ndk, args.target_dir, args.jobs)
    env["NVF_PYTHON"] = sys.executable
    if args.gradle_build_dir:
        args.gradle_build_dir = args.gradle_build_dir.resolve()
        env["NVF_ANDROID_BUILD_DIR"] = str(args.gradle_build_dir)
    if args.tests and not args.check:
        parser.error("--tests requires --check; Android test execution needs a device")
    package = "nvforever" if args.tests else "nvf-android"
    command = ["cargo", "check" if args.check else "build", "--manifest-path", "engine/Cargo.toml",
               "-p", package, "--lib", "--target", TARGET, "--locked"]
    if args.tests:
        command.append("--tests")
    if not args.check:
        command.extend(["--profile", "android-dev"])
    subprocess.run(command, cwd=ROOT, env=env, check=True)
    if args.check:
        return
    library = args.target_dir / TARGET / "android-dev/libnvf_android.so"
    libraries = ROOT / "native-libs/arm64-v8a"
    libraries.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(library, libraries / library.name)
    # Some C++ dependencies use the NDK's shared runtime; package the matching one.
    host = {"Windows": "windows-x86_64", "Linux": "linux-x86_64", "Darwin": "darwin-x86_64"}[platform.system()]
    runtime = args.ndk / f"toolchains/llvm/prebuilt/{host}/sysroot/usr/lib/{TARGET}/libc++_shared.so"
    shutil.copyfile(runtime, libraries / runtime.name)
    assets = ROOT / "runtime-assets"
    if assets.exists():
        # Only this generated directory is replaced, not any source or game folder.
        if assets.is_symlink() or assets.resolve().parent != ROOT.resolve():
            raise SystemExit("Refusing to replace an assets directory outside this checkout")
        shutil.rmtree(assets)
    stage_assets(assets)
    gradle = shutil.which(args.gradle) or args.gradle
    command = [gradle, "--no-daemon", "--max-workers=2"]
    if args.gradle_build_dir:
        command.extend(["--project-cache-dir", str(args.gradle_build_dir / "project-cache")])
    # Incremental APK updates can retain an obsolete native payload outside the
    # active ZIP directory. Recreate app packaging, preserving the Rust cache.
    command.extend([":app:clean", ":app:assembleDebug"])
    subprocess.run(command, cwd=ROOT / "android", env=env, check=True)
    print((args.gradle_build_dir / "app" if args.gradle_build_dir else ROOT / "android/app/build") / "outputs/apk/debug/app-debug.apk")


if __name__ == "__main__":
    main()
