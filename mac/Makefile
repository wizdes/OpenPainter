# Build, test and measure the Mac app. Run from the repo root as `make -C mac`.
#   make -C mac            the Rust staticlib, the Xcode project, then the app (Release)
#   make -C mac test       the XCTests (unit tests, view tests and the 30 s trace gate);
#                          builds opcli first for the M28 scenario (Phase2EndToEndTests)
#   make -C mac launch-time  scripts/launch-time.sh on the Release app
#   make -C mac trace      `OpenPainter --trace` on a generated 6000x4000 PNG
# Order: engine (cargo build -p op-ffi --release) -> xcodegen -> xcodebuild.
DERIVED := build/DerivedData
APP := $(DERIVED)/Build/Products/Release/OpenPainter.app
XCB := xcodebuild -project OpenPainter.xcodeproj -scheme OpenPainter -derivedDataPath $(DERIVED)

.PHONY: all engine opcli project app test launch-time trace clean
all: app

engine:
	scripts/build-engine.sh

opcli:
	scripts/build-opcli.sh

project:
	xcodegen generate --spec project.yml --quiet

app: engine project
	$(XCB) -configuration Release build

test: engine opcli project
	$(XCB) -configuration Debug test

launch-time: app
	../scripts/launch-time.sh $(APP)

trace: app
	$(APP)/Contents/MacOS/OpenPainter --make-trace-png build/trace-6000x4000.png
	$(APP)/Contents/MacOS/OpenPainter --trace build/trace-6000x4000.png

clean:
	rm -rf build OpenPainter.xcodeproj
