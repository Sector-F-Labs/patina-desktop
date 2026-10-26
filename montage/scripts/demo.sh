#!/bin/sh
# Makes demo media with ffmpeg's test sources, and a project that uses them,
# in target/demo (or $1): for trying Montage out, its screenshots and the
# export test, without anyone's own videos.
set -eu
cd "$(dirname "$0")/.."
dir=${1:-target/demo}
mkdir -p "$dir"
ff() { ffmpeg -hide_banner -loglevel error -y "$@"; }
ff -f lavfi -i "testsrc2=size=1280x720:rate=30:duration=8" -f lavfi -i "sine=frequency=330:duration=8" \
   -c:v libx264 -pix_fmt yuv420p -c:a aac -shortest "$dir/Morning.mp4"
ff -f lavfi -i "mandelbrot=size=1280x720:rate=30,trim=duration=6" -f lavfi -i "sine=frequency=440:duration=6" \
   -c:v libx264 -pix_fmt yuv420p -c:a aac -shortest "$dir/Garden.mp4"
ff -f lavfi -i "smptehdbars=size=960x720:rate=30:duration=5" \
   -c:v libx264 -pix_fmt yuv420p "$dir/Old Camera.mp4"
ff -f lavfi -i "aevalsrc=0.2*sin(2*PI*220*t)*(0.6+0.4*sin(2*PI*t)):s=48000:d=30" -c:a flac "$dir/Song.flac"
ff -f lavfi -i "aevalsrc=0.5*sin(2*PI*180*t)*gt(sin(2*PI*3*t)\,0):s=48000:d=4" -c:a flac "$dir/Voice.flac"
abs=$(cd "$dir" && pwd)
cat > "$dir/Demo.montage" <<JSON
{
  "format": 1,
  "media": [
    { "id": 1, "path": "$abs/Morning.mp4" },
    { "id": 2, "path": "$abs/Garden.mp4" },
    { "id": 3, "path": "$abs/Old Camera.mp4" },
    { "id": 4, "path": "$abs/Song.flac" },
    { "id": 5, "path": "$abs/Voice.flac" }
  ],
  "next_id": 6,
  "video": [
    { "media": 1, "from": 1.0, "length": 6.0 },
    { "media": 3, "from": 0.0, "length": 4.0 },
    { "media": 2, "from": 0.5, "length": 5.0 }
  ],
  "music": [ { "at": 0.0, "clip": { "media": 4, "from": 0.0, "length": 15.0, "volume": 0.5 } } ],
  "voice": [ { "at": 6.5, "clip": { "media": 5, "from": 0.0, "length": 4.0 } } ]
}
JSON
echo "$dir/Demo.montage"
