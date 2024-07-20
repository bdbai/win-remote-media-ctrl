# win-remote-media-ctrl

Remotely control your Windows PC's media playback via LAN. Using a private key authentication to prevent replay attacks even through an unreliable channel.

## Usage

1. Prepare a 512-bit private key.
```bash
cat /dev/urandom | head -c 64 | base64 > private_key.txt
```
2. Run the server from project root.
```bash
cargo run --release
```
3. Open `https://127-0-0-1.traefik.me:9201/` in your browser. Replace `127-0-0-1` with your server's IP address.

> [!NOTE]
> Web Crypto API requires a secure context, therefore we abuse [traefik.me](https://github.com/pyrou/traefik.me) to cheat the browser. During server startup, the pem files from traefik.me are downloaded automatically. Use it at your own risk.

4. Paste the private key into the input field under `Debug` section. It should take effect immediately.

5. Play around with the buttons.
