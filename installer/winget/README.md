# winget

The Windows Package Manager manifests for MagicInput, laid out like `microsoft/winget-pkgs`
(`manifests/b/BullfighterStudio/MagicInput/<version>/`). To publish a version, copy its folder into a
fork of `microsoft/winget-pkgs` and open a pull request there (or run
`wingetcreate update BullfighterStudio.MagicInput --version X.Y.Z --urls <setup url> --submit`).

Check a version on a PC first: `winget validate --manifest <folder>`. The installer hash is the
`MagicInput-Setup.exe` line of the release's `SHA256SUMS.txt`.
