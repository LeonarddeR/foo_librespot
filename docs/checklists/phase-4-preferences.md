# Phase 4 check: logging in, the preferences page and error reporting

This checklist covers phase 4 in foobar2000 2.26 x64:

- logging in from the preferences page;
- the login window that opens when a link needs a login;
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

## Already checked by the agent (2026-11-04, foobar2000 2.26.2 x64 portable)

These were checked by reading foobar2000's windows through UI Automation (`winapp ui`) and capturing them. The Windows session was locked, so no keys could be pressed.

### The preferences page

- [x] File > Preferences > Tools lists "foo_librespot", and selecting it opens the page without an error.
- [x] Each control has the name and role a screen reader announces:
  - Group "Account".
  - Text "Logged in to Spotify as …".
  - Button "Log out".
  - Text "Code and page:", then the read-only field. Its name is "Code and page:" and its value is "No login in progress.".
  - Button "Open browser", disabled.
  - Group "Playback".
  - Text "Preferred bitrate:", then the combo box. Its name is "Preferred bitrate:" and its value is "320 kbit/s".
- [x] The layout is clean: both group boxes enclose their controls, labels line up with their fields, and nothing is clipped.
- [x] An existing login shows as "Logged in to Spotify as …" when the page opens.

### Logging in on first use

These were run with the stored login moved aside; it was restored byte-for-byte afterwards.

- [x] Adding the album while logged out (`foobar2000.exe /add spotify:album:4wPtS24asgBAG4VOmYYB9O`) opens the "Log in to Spotify" window. foobar2000's "Processing Files" waits behind it.
- [x] The window gets a real code from Spotify. The status reads "Press Open browser, or open the page below yourself, and confirm the code there." The code field holds the code and the page, and Open browser is enabled.
- [x] No browser opened by itself.
- [x] The window's controls, in order: status, "Cancel login", "Code and page:" with its field, "Open browser", "Close". The layout is clean, and the two-line status fits.
- [x] Pressing Close cancels the login and closes the window. foobar2000 then reports: "Not logged in to Spotify. Log in from File > Preferences > Tools > foo_librespot."

### The console

- [x] At startup, with foo_youtube loaded: "foo_librespot: foo_youtube takes open.spotify.com links before foo_librespot sees them. Add the line "deny spotify.com" to foo_youtube's host filter in its preferences; spotify: links work either way."
- [x] Once per session while logged out: "foo_librespot: not logged in to Spotify; log in from File > Preferences > Tools > foo_librespot."

### Found

- The status shows the Spotify username, which for newer accounts is a random-looking id such as "21e2w4ryeu3jowufvdh5ctyii". A friendlier name would need the Web API, which this component doesn't use.
- Setting the combo box through UI Automation doesn't enable Apply. Windows doesn't notify a dialog of programmatic selections; choosing with the keyboard or mouse does (step 3 below).

## For you, with NVDA

Each step lists what you should hear.

1. **Tab order on the page.**
   - Open File > Preferences, go to Tools > foo_librespot, and press Tab from the page tree.
   - You should hear the account button, then "Code and page:" with its text, then "Preferred bitrate:" with its value.
   - "Open browser" is skipped while no login is waiting.
   - Alt+L, Alt+D and Alt+P (the underlined letters) reach the button, the code field and the bitrate.
2. **Logging out and in.**
   - Press Log out. The button becomes "Log in", and NVDA reads "Not logged in to Spotify." without focus moving.
   - Press Log in. NVDA reads "Asking Spotify for a login code…", then "Press Open browser, or open the page below yourself, and confirm the code there.".
   - Nothing opens by itself.
   - Tab to the code field and read the code. Tab on to Open browser and press it: Spotify's page opens with the code filled in.
   - Confirm. NVDA reads "Logged in to Spotify as …" without focus moving.
3. **The bitrate.**
   - Choose 160 kbit/s with the arrow keys. Apply becomes available.
   - Press Apply. Close Preferences, open it again, and the page shows 160 kbit/s.
   - Set it back to 320 kbit/s.
4. **Cancelling.** Press Log in, then press the same button again ("Cancel login"). NVDA reads "Not logged in to Spotify."
5. **The login window.**
   - Log out on the page and close Preferences.
   - Paste the album link into a playlist. The "Log in to Spotify" window opens with focus on the code field, and no browser opens.
   - Tab reaches Open browser and Close.
   - Confirm the login: NVDA reads "Logged in to Spotify as …" and the album's 11 tracks arrive. Close the window with Escape.
6. **Cancelling from the window.**
   - Log out again and paste the link again.
   - Press Escape. The window closes, and foobar2000 says to log in from Preferences.
   - Log in again from the page afterwards.
7. **The skipped-items popup.**
   - Add a playlist that holds a podcast episode or a local file.
   - One popup says how many tracks were added and lists what was skipped, with the reason for each.
