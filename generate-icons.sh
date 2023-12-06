#!/bin/bash
SRC_FILE="./img/logo.png"  # Replace with the path to your source image file if different
DST_PATH="./src-tauri/icons"         # Destination path for your icons

# Function to log info messages
info() {
  echo "Info: $1"
}

info 'Generating Icons'

# Create the icons directory if it doesn't exist
mkdir -p "$DST_PATH"

info 'Generating icons/32x32.png ...'
convert "$SRC_FILE" -resize 32x32 "$DST_PATH/32x32.png"

info 'Generating icons/128x128.png ...'
convert "$SRC_FILE" -resize 128x128 "$DST_PATH/128x128.png"

info 'Generating icons/128x128@2x.png (256x256) ...'
convert "$SRC_FILE" -resize 256x256 "$DST_PATH/128x128@2x.png"

info 'Generating icons/icon.icns ...'
mkdir -p "$DST_PATH/icon.iconset"
convert "$SRC_FILE" -resize 16x16 "$DST_PATH/icon.iconset/icon_16x16.png"
convert "$SRC_FILE" -resize 32x32 "$DST_PATH/icon.iconset/icon_16x16@2x.png" # icon_16x16@2x is effectively 32x32
convert "$SRC_FILE" -resize 32x32 "$DST_PATH/icon.iconset/icon_32x32.png"
convert "$SRC_FILE" -resize 64x64 "$DST_PATH/icon.iconset/icon_32x32@2x.png" # icon_32x32@2x is effectively 64x64
convert "$SRC_FILE" -resize 128x128 "$DST_PATH/icon.iconset/icon_128x128.png"
convert "$SRC_FILE" -resize 256x256 "$DST_PATH/icon.iconset/icon_128x128@2x.png"
convert "$SRC_FILE" -resize 256x256 "$DST_PATH/icon.iconset/icon_256x256.png"
convert "$SRC_FILE" -resize 512x512 "$DST_PATH/icon.iconset/icon_256x256@2x.png"
convert "$SRC_FILE" -resize 512x512 "$DST_PATH/icon.iconset/icon_512x512.png"
convert "$SRC_FILE" -resize 1024x1024 "$DST_PATH/icon.iconset/icon_512x512@2x.png"
iconutil -c icns "$DST_PATH/icon.iconset" -o "$DST_PATH/icon.icns"
rm -r "$DST_PATH/icon.iconset"

info 'Generating icons/icon.ico ...'
convert "$SRC_FILE" -resize 256x256 "$DST_PATH/icon.ico"

info 'Done generating icons.'
