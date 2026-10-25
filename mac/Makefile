# Build, test and measure the Mac app. Run from the repo root as `make -C mac`.
#   make -C mac            the Rust staticlib, the Xcode project, then the app (Release)
#   make -C mac test       the XCTests (unit tests, view tests and the 30 s trace gate);
#                          builds opcli first for the M28 scenario (Phase2EndToEndTests)
#   make -C mac launch-time  scripts/launch-time.sh --first-effect on the Release app
#   (scripts/perf-check.sh runs every speed and size gate, M58)
#   make -C mac trace      `OpenPainter --trace` on a generated 6000x4000 PNG
# Order: engine (scripts/build-engine.sh) -> xcodegen -> xcodebuild.
DERIVED := build/DerivedData
APP := $(DERIVED)/Build/Products/Release/OpenPainter.app
# M58: Swift packages resolve only to the versions in the tracked
# mac/Package.resolved (Sparkle's exact revision), which `project` copies into
# the generated project. After changing a package in project.yml, run
# `make -C mac resolve` and commit the new Package.resolved.
RESOLVED := OpenPainter.xcodeproj/project.xcworkspace/xcshareddata/swiftpm/Package.resolved
XCB := xcodebuild -project OpenPainter.xcodeproj -scheme OpenPainter -derivedDataPath $(DERIVED) -onlyUsePackageVersionsFromResolvedFile

.PHONY: all engine opcli project resolve app test launch-time trace clean
all: app

engine:
	scripts/build-engine.sh

opcli:
	scripts/build-opcli.sh

project:
	xcodegen generate --spec project.yml --quiet
	mkdir -p $(dir $(RESOLVED))
	cp Package.resolved $(RESOLVED)

resolve:
	xcodegen generate --spec project.yml --quiet
	rm -f $(RESOLVED)
	xcodebuild -project OpenPainter.xcodeproj -scheme OpenPainter -derivedDataPath $(DERIVED) -resolvePackageDependencies
	cp $(RESOLVED) Package.resolved

app: engine project
	$(XCB) -configuration Release build

test: engine opcli project
	$(XCB) -configuration Debug test

launch-time: app
	../scripts/launch-time.sh --first-effect $(APP)

trace: app
	$(APP)/Contents/MacOS/OpenPainter --make-trace-png build/trace-6000x4000.png
	$(APP)/Contents/MacOS/OpenPainter --trace build/trace-6000x4000.png

clean:
	rm -rf build OpenPainter.xcodeproj
