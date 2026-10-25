CI fills this directory with validated, immutable plugin releases before the
Docker build. `prepare-image-package.ps1` builds a new version or reuses its
`dalamud-v<version>` GitHub prerelease. Generated packages are ignored by Git.

The Docker runtime reads `current.json` and the version directories here. A local
image built without those files does not advertise an installation or download.
