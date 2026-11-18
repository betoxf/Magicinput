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
- Pairing uses a password-authenticated key exchange (symmetric SPAKE2 over Ed25519, bound to the protocol version). The pairing code is never transmitted, and recording a connection doesn't let anyone test guesses of the code offline: each connection attempt can check at most one guess.
- The key from that exchange, through HKDF-SHA256 over the exchange transcript, is the pre-shared key of the Noise protocol (`Noise_NNpsk0_25519_ChaChaPoly_BLAKE2s`) that encrypts everything afterwards. Without the code a device can't complete the handshake.
- Discovery (mDNS) advertises only a random computer id, the computer's name and the protocol version; nothing derived from the pairing code.
- Failed pairing attempts are paced: a few per address, then a growing wait between attempts, plus an overall limit per minute for addresses that never paired. Failed attempts are logged without the code.
- Only peers on the same local subnet are accepted or dialed; connections from the internet, VPNs and Tailscale addresses are refused.
- Configuration (including the pairing code) is stored unencrypted in your user profile.
- Users are responsible for the security of their operating system accounts and local network.

See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for the full model.
