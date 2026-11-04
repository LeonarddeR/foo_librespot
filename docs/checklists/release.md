# Release check: the package and the release

This checklist proves that the package CI builds installs and works in a real foobar2000, and then walks through the release itself.
Run it before every release.

## The package

Use the `component` artifact of the latest CI run on `main`:

```
gh run download <run id> -n component -D <folder>
```

1. - [ ] With the portable foobar2000 closed, remove `.fb2k-portable\profile\user-components-x64\foo_librespot`, so that only the package installs the component.
2. - [ ] Start the portable foobar2000 (`pwsh tools/fb2k-portable.ps1 -Run`). In File > Preferences > Components, press Install and choose `foo_librespot-<version>.fb2k-component`. Press OK and let foobar2000 restart.
3. - [ ] Components lists "Spotify integration using librespot" with the version being released.
4. - [ ] The component's folder, `.fb2k-portable\profile\user-components-x64\foo_librespot`, holds `foo_librespot.dll`, `LICENSE.txt` and `licenses.html`. `licenses.html` opens in a browser, with one heading per licence.
5. - [ ] Adding the Keith & Kristyn Getty album `https://open.spotify.com/album/4wPtS24asgBAG4VOmYYB9O` adds its 11 tracks, and "There Is A Higher Throne" plays.

## The release

6. - [ ] `cargo release` (a dry run) shows one CHANGELOG.md change dating the unreleased section, one `Release <version>` commit, one `v<version>` tag, and a push of `main` and the tag.
7. - [ ] `cargo release --execute` runs the checks, commits, tags and pushes.
8. - [ ] The CI run for the tag passes, and its Release job publishes the GitHub release.
9. - [ ] The release on `https://github.com/LeonarddeR/foo_librespot/releases` is marked as a pre-release (for 0.x), its notes are the changelog section for the version, and it has `foo_librespot-<version>.fb2k-component` and `foo_librespot.pdb`.

## Result

Tested by: …
Version: …
Outcome: …
