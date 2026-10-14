# Security policy

MagicInput sees and replays keyboard and mouse input and clipboard text between two computers, so security reports are handled privately.

## Supported versions

| Version | Support |
| --- | --- |
| Latest release | Security fixes and investigation |
| Older releases | Best effort; upgrade may be required |

## Reporting a vulnerability

Do not open a public issue for a suspected vulnerability. Use GitHub's private vulnerability reporting from the repository **Security** tab. If that isn't available, contact the repository owner privately through their GitHub profile before sharing details.

Include the MagicInput, macOS and Windows versions, your network setup, reproduction steps, the expected security boundary, what you observed, and a minimal proof of concept. Never send real pairing codes or captured traffic from your own computers.

## Security boundaries

- There is no MagicInput server; the two computers talk directly.
- Every connection is encrypted with the Noise protocol (`Noise_NNpsk0_25519_ChaChaPoly_BLAKE2s`); the pre-shared key is derived from the pairing code, which is never transmitted. Without the code a device can't complete the handshake.
- Only peers on the same local subnet are accepted or dialed; connections from the internet, VPNs and Tailscale addresses are refused.
- The pairing code is short (about 39 bits) so it's easy to type. Someone on your local network who records a MagicInput connection could recover it by offline guessing and then connect. The local-network restriction is the main boundary; on untrusted networks (cafés, shared offices), generate a new code afterwards. Moving to a password-authenticated key exchange (PAKE), which removes offline guessing, is planned.
- Configuration (including the pairing code) is stored unencrypted in your user profile.
- Users are responsible for the security of their operating system accounts and local network.

See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for the full model.
