# Phase 4 check: logging in, the preferences page and error reporting

This checklist covers phase 4 in foobar2000 2.26 x64:

- logging in from the preferences page;
- the login window that opens when a link or a play needs a login;
- the bitrate preference;
- the messages that explain what went wrong.

It uses the portable foobar2000 from `tools/fb2k-portable.ps1`, so your normal profile is not touched.

## Setup

```
pwsh tools/fb2k-portable.ps1 -Install foo_librespot
pwsh tools/fb2k-portable.ps1 -Run
```

Test links, all by Keith & Kristyn Getty:

- Album "In Christ Alone" (11 tracks): `spotify:album:4wPtS24asgBAG4VOmYYB9O`
- Playlist "Give Us This Day Our Daily Hymns" (35 tracks, nothing skipped): `spotify:playlist:5cQ8xIDb1NvIsEPjHgHg55`

Links copied from the Spotify app or a browser, such as `https://open.spotify.com/album/…?si=…`, work too.

## Already checked by the agent (2026-11-04, foobar2000 2.26.2 x64 portable)

These were checked by reading foobar2000's windows through UI Automation (`winapp ui`) and capturing them. The Windows session was locked for most of them, so no keys could be pressed.

### The preferences page

- [x] File > Preferences > Tools lists "foo_librespot", and selecting it opens the page without an error.
- [x] Each control has the name and role a screen reader announces:
  - Group "Account".
  - Text "Logged in to Spotify as …".
  - Button "Log out".
  - Group "Playback".
  - Text "Preferred bitrate:", then the combo box. Its name is "Preferred bitrate:" and its value is "320 kbit/s".
- [x] While logged in, the page has no code field, page field or Open browser button.
- [x] The layout is clean: both group boxes enclose their controls, labels line up with their fields, and nothing is clipped.
- [x] An existing login shows as "Logged in to Spotify as …" when the page opens.
- [x] The Preferences window's own buttons (Reset all, Reset page, OK, Cancel, Apply) have no access keys, so the page's access keys can't clash with them.

### Logging in on first use

These were run with the stored login moved aside; it was restored byte-for-byte afterwards. They were run before the code field became two fields and before the window closed itself.

- [x] Adding the album while logged out (`foobar2000.exe /add spotify:album:4wPtS24asgBAG4VOmYYB9O`) opens the "Log in to Spotify" window. foobar2000's "Processing Files" waits behind it.
- [x] The window gets a real code from Spotify. The status reads "Press Open browser, or open the page below yourself, and confirm the code there." Open browser is enabled.
- [x] No browser opened by itself.
- [x] Pressing Close cancels the login and closes the window. foobar2000 then reports: "Not logged in to Spotify. Log in from File > Preferences > Tools > foo_librespot."

### Links

- [x] `foobar2000.exe /add "https://open.spotify.com/album/3Zh2jUjwABpVsX2IRRAHDN?si=qnk_JX7UShqydK0CkpQKFA"` adds the album's 13 tracks, with foo_youtube loaded.

### The console

- [x] At startup, with foo_youtube loaded: "foo_librespot: foo_youtube may take open.spotify.com links before foo_librespot sees them. Add the line "deny spotify.com" to foo_youtube's host filter in its preferences; spotify: links work either way."
- [x] Once per session while logged out: "foo_librespot: not logged in to Spotify; log in from File > Preferences > Tools > foo_librespot."

### Found

- The status shows the Spotify username, which for newer accounts is a random-looking id such as "21e2w4ryeu3jowufvdh5ctyii". A friendlier name would need the Web API, which this component doesn't use.
- Setting the combo box through UI Automation doesn't enable Apply. Windows doesn't notify a dialog of programmatic selections; choosing with the keyboard or mouse does (step 3 below).
- Found by the maintainer with NVDA, and fixed:
  - Escape did nothing in the code field: it was a multi-line edit, which keeps Escape and Enter for itself. The code and the page are now two single-line fields.
  - The code field was always shown. On the page, the code, the page and Open browser now appear only while a login waits.
  - The login window stayed open after the login succeeded. It now closes by itself.
  - The status bar showed "?" for the bitrate. It now shows the codec and bitrate of the file that plays.

## For you, with NVDA

Each step lists what you should hear.

1. **Tab order on the page, logged in.**
   - Open File > Preferences, go to Tools > foo_librespot, and press Tab from the page tree.
   - You should hear "Log out", then "Preferred bitrate:" with its value. Nothing else: there is no login waiting.
   - Alt+O reaches Log out and Alt+P the bitrate.
2. **Logging out and in.**
   - Press Log out. The button becomes "Log in", and NVDA reads "Not logged in to Spotify." without focus moving.
   - Press Log in. NVDA reads "Asking Spotify for a login code…", then "Press Open browser, or open the page below yourself, and confirm the code there.".
   - Nothing opens by itself.
   - Tab: you hear "Code:" with the code, then "Page:" with the address, then "Open browser". Alt+D and Alt+G reach the two fields.
   - In the code field, press Escape: Preferences closes. Open it again; the login is still waiting and the page shows the same code.
   - Tab to Open browser and press it: Spotify's page opens with the code filled in.
   - Confirm in the browser, then go back to foobar2000 with Alt+Tab. Focus is on the button, which now reads "Log out", and the status reads "Logged in to Spotify as …". The code, the page and Open browser are gone.
3. **The bitrate.**
   - Choose 160 kbit/s with the arrow keys. Apply becomes available.
   - Press Apply and close Preferences.
   - Play a Spotify track. The status bar shows "Vorbis" and "160 kbps".
   - Open Preferences again: the page shows 160 kbit/s. Set it back to 320 kbit/s and press Apply.
   - Play another track: the status bar shows "320 kbps". A track that was already playing keeps its bitrate until it is played again.
4. **Cancelling.** Press Log in, then press the same button again ("Cancel login"). NVDA reads "Not logged in to Spotify.", the code fields disappear, and focus stays on the button. Log in again afterwards.
5. **The login window, from a link.**
   - Log out on the page and close Preferences.
   - Paste the album link into a playlist. The "Log in to Spotify" window opens with focus on the "Code:" field, which reads "Waiting for Spotify's code…" until the code arrives. No browser opens.
   - Once the code is there, Tab reaches Page, Open browser, Close and Cancel login. Open browser only appears with the code.
   - Confirm the login. The window closes by itself and the album's 11 tracks arrive.
6. **The login window, from playing.**
   - Log out on the page and close Preferences.
   - Play one of the Spotify tracks. The login window opens.
   - Confirm the login. The window closes by itself and the track starts playing.
7. **Cancelling from the window.**
   - Log out again and paste the link again.
   - Press Escape. The window closes, and foobar2000 says to log in from Preferences.
   - Play a Spotify track. It fails at once with the same advice, and no window opens: you cancelled, so foo_librespot doesn't ask again for each track. Adding a link or pressing Log in asks again.
   - Log in again from the page afterwards.

## How to see each message

foobar2000 shows errors from adding a link in the "Processing Files" window. It reports errors from playing a track itself. Both also go to the console (View > Console).

- **"Not logged in to Spotify. Log in from File > Preferences > Tools > foo_librespot."** Log out, add a link, and close the login window with Escape (step 7). Playing a track after that shows it too.
- **The once-per-session console line "foo_librespot: not logged in to Spotify; …".** The same as above; it appears once per run of foobar2000.
- **The skipped-items popup.** Add a Spotify playlist that holds something foo_librespot can't add: a podcast episode, a local file from the Spotify app, or a track that isn't available in your country. In the Spotify app, add a podcast episode and a Getty song to a new playlist, then add that playlist's link. One popup says "Added 1 track from …" and lists the episode with the reason. The console lists every skipped item.
- **"Spotify: …" when the network is gone.** Turn off the network and play a Spotify track that hasn't played yet. foobar2000 reports the error instead of hanging; Stop works at once.
- **The foo_youtube advice.** Start foobar2000 with foo_youtube installed and open View > Console.
- **"This account isn't Premium".** Only a free Spotify account shows it.
