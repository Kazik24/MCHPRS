# MCHPRS waveform demo

This package runs the real [VaporView VS Code extension](https://github.com/Lramseyer/vaporview) in browser-based code-server. It opens a static VCD fixture so the extension UI can be reviewed; there is no MCHPRS capture or live server integration.

The image pins code-server 4.104.3 (bundled Code 1.104.3) and VaporView 1.5.4. VaporView declares a VS Code `^1.102.0` engine requirement. The image build needs network access to pull code-server and install the pinned extension from Open VSX. The sample VCD itself is local and needs no network.

From this directory, set a local password and start the service:

```powershell
$env:WAVEFORM_PASSWORD = "choose-a-local-password"
docker compose up --build
```

Open [http://localhost:8443/?folder=/home/coder/project&payload=%5B%5B%22openFile%22%2C%22%2Fhome%2Fcoder%2Fproject%2Fmchprs-demo.vcd%22%5D%5D](http://localhost:8443/?folder=/home/coder/project&payload=%5B%5B%22openFile%22%2C%22%2Fhome%2Fcoder%2Fproject%2Fmchprs-demo.vcd%22%5D%5D) and enter the password. The workspace associates `*.vcd` with VaporView. If your browser does not open the sample automatically, open `mchprs-demo.vcd` from the Explorer.

The fixture has a clock, a four-bit dust-strength signal, a gate signal, a missing (`x`) sample, and several same-timestamp strength changes whose order is preserved in the VCD file. Coordinates are labels only. The fixture's `1 ns` unit is an arbitrary VCD time unit for rendering; it is not calibrated to Minecraft ticks or physical duration. VaporView's glitch-removal option is disabled for this workspace.

The workspace mount is read-only and the service binds to loopback on port 8443. The password is required and must be supplied through `WAVEFORM_PASSWORD`; `.env` is ignored if you prefer to store it locally. Stop with `docker compose down`.
