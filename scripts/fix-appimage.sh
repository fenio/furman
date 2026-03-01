#!/usr/bin/env bash
# Strip bundled WebKit and GStreamer libraries from the AppImage.
#
# linuxdeploy copies WebKit and GStreamer into the AppImage, but the
# bundled copies conflict with system libraries on newer distros
# (e.g. Debian 13). These should always come from the system.
#
# Usage: ./scripts/fix-appimage.sh [path-to.AppImage]

set -euo pipefail

APPIMAGE="${1:-$(ls src-tauri/target/release/bundle/appimage/Furman_*.AppImage 2>/dev/null | head -1)}"

if [ -z "$APPIMAGE" ] || [ ! -f "$APPIMAGE" ]; then
  echo "Error: AppImage not found. Pass the path as an argument."
  exit 1
fi

echo "Fixing AppImage: $APPIMAGE"

# Extract
"$APPIMAGE" --appimage-extract > /dev/null 2>&1

# Remove bundled WebKit libraries
echo "Removing bundled WebKit..."
rm -f squashfs-root/usr/lib/libwebkit2gtk-4.1* \
      squashfs-root/usr/lib/libjavascriptcoregtk-4.1*
rm -rf squashfs-root/usr/lib/webkit2gtk-4.1/

# Remove bundled GStreamer (system WebKit needs matching system GStreamer)
echo "Removing bundled GStreamer..."
rm -f squashfs-root/usr/lib/libgst*
rm -rf squashfs-root/usr/lib/gstreamer-1.0/

echo "Removed bundled WebKit and GStreamer — AppImage will use system libs."

# Repackage using appimagetool
if ! command -v appimagetool &> /dev/null; then
  echo ""
  echo "appimagetool not found. Install it to repackage:"
  echo "  wget https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-x86_64.AppImage"
  echo "  chmod +x appimagetool-x86_64.AppImage"
  echo "  sudo mv appimagetool-x86_64.AppImage /usr/local/bin/appimagetool"
  echo ""
  echo "Then repackage manually:"
  echo "  appimagetool squashfs-root $APPIMAGE"
  exit 0
fi

appimagetool squashfs-root "$APPIMAGE"
rm -rf squashfs-root
echo "Done: $APPIMAGE"
