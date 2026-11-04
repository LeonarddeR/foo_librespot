# Check: podcast episodes and shows

This checklist covers episode and show links in foobar2000 2.26 x64, in the portable foobar2000 from `tools/fb2k-portable.ps1`.

## Setup

```
pwsh tools/fb2k-portable.ps1 -Install foo_librespot
pwsh tools/fb2k-portable.ps1 -Run
```

Test link: the episode "Our Ransom (Part 1)" of the Timothy Keller Sermons Podcast, which Spotify hosts itself:
`https://open.spotify.com/episode/4QomYTPgZ1mpVbUmxBsUpz?si=e9wYxjRdSRGHEGfuaT3DZg&utm_source=whatsapp`

## Already checked by the agent (2026-11-04)

- [x] `foobar2000.exe /add spotify:episode:4QomYTPgZ1mpVbUmxBsUpz` adds one entry, "Our Ransom (Part 1)", with "Timothy Keller Sermons Podcast by Gospel in Life" as artist and album. The console says "foo_librespot: added 1 track from the link".
- [x] The `spotify_dev` example resolves the episode (37:39, hosted by Spotify) and decodes its first five seconds. Spotify serves it as 96 kbit/s Ogg Vorbis, the only quality it has for this episode.

Not checked yet: a show link (Spotify's web page doesn't show the show's id without a browser), and an episode hosted outside Spotify.

## For you, with NVDA

1. **An episode link.** Paste the test link into a playlist. One entry arrives. Play it: it starts, seeking works, and the status bar shows "Vorbis" and "96 kbps", whatever the preferred bitrate.
2. **The episode's info.** Open its Properties. The title is "Our Ransom (Part 1)", artist and album are the show, the genre is Podcast, and the comment holds the episode's description.
3. **A show.** Copy a show's link from the Spotify app (Share > Copy link) and paste it. All its episodes arrive in the order Spotify lists them, and the console says how many were added.
4. **An episode Spotify doesn't host.** If a podcast you know streams from its own server, add one of its episodes. The entry's path is the podcast's own audio link, not `sptf://`, and it plays through foobar2000's own decoders, with the episode's title, show and date.
5. **A playlist with an episode.** Add a Spotify playlist that holds an episode. The episode is added in its place instead of being skipped.
