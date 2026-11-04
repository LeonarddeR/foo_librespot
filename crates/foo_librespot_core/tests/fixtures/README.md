# Test fixtures

Small Ogg Vorbis files for the decoder tests, made with ffmpeg (any build with libvorbis):

- `chirp.ogg`: 2 s, 44.1 kHz stereo. The left channel is a chirp from 200 Hz upwards, so no two
  stretches look alike and an off-by-one seek shows up; the right channel is silent.

  ```
  ffmpeg -f lavfi -i "aevalsrc=exprs='0.3*sin(2*PI*(200+400*t)*t)|0':s=44100:d=2" -c:a libvorbis -q:a 0 -map_metadata -1 -fflags +bitexact -flags:a +bitexact chirp.ogg
  ```

- `mono22k.ogg`: the same chirp, mono at 22.05 kHz, which the decoder must reject.

  ```
  ffmpeg -f lavfi -i "aevalsrc=exprs='0.3*sin(2*PI*(200+400*t)*t)':s=22050:d=2" -ac 1 -ar 22050 -c:a libvorbis -q:a 0 -map_metadata -1 -fflags +bitexact -flags:a +bitexact mono22k.ogg
  ```
