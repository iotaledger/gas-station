# Docker Images Management Workflow

## Parameters

| Parameter              | Description                                                                                     |
|------------------------|-------------------------------------------------------------------------------------------------|
| **Source tag**         | The existing tag you want to retag from (e.g., `latest`, `0.2.0`)                               |
| **Target tags**        | Comma-separated list of new tags (e.g., `0.2.1,stable,release`). **Leave empty and enable "Remove source image" to only remove the source tag** |
| **Repository**         | Choose between `iotaledger/gas-station` or `iotaledger/gas-station-tool`                       |
| **Dry run**            | Enable to test without actually pushing (recommended for testing)                              |
| **Remove source image**| Optionally remove the source image from Docker Hub after successful retagging                  |

## Examples

### Fix a mis-tagged image

- Source tag: `0.2.0`
- Target tags: `0.2.0-fixed,stable`
- Repository: `iotaledger/gas-station-tool`

### Retag and clean up old version

- Source tag: `0.1.9`
- Target tags: `0.2.0,latest`
- Repository: `iotaledger/gas-station`
- Remove source image: `true` (removes `0.1.9` from Docker Hub)

### Remove a specific tag only

- Source tag: `0.1.9`
- Target tags: (leave empty)
- Repository: `iotaledger/gas-station`
- Remove source image: `true` (removes `0.1.9` from Docker Hub)

### No operation (nothing to do)

- Source tag: `0.1.9`
- Target tags: (leave empty)
- Repository: `iotaledger/gas-station`
- Remove source image: `false` (no operation performed)
