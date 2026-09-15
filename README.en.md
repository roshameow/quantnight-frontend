# QuantNight

[中文](README.md) · [Download](https://github.com/roshameow/quantnight-frontend/releases/latest) · [User guide](docs/user-guide.en.md)

A desktop console for quantitative research tasks: manage regular, Priority and Super jobs, track local and remote execution, and explore results.

![QuantNight task manager demo](assets/demo.gif)

## Try the interface

Run an interactive demo with synthetic data. No database, platform account or private backend is required.

```bash
git clone https://github.com/roshameow/quantnight-frontend.git
cd quantnight-frontend
npm ci
npm run demo
```

Open `/demo.html` at the address printed by Vite. Browse task cards, toggle local/remote labels and filter sample results. Start, pause and delete actions change in-memory demo data only; refresh resets it. No simulations are submitted. Requires Node.js 22 or later.

## Scope

| Component | Availability |
| --- | --- |
| Vue + Tauri desktop UI | Public source in this repository |
| Task, configuration and result views | Desktop use requires configured MongoDB; the demo does not |
| Simulation execution, task generation and remote scripts | Separate private `quantnight` backend; not included |

The desktop download does not include the execution backend. The full workflow requires a compatible backend, script mappings and the appropriate platform account. Demo results are fictional and are not evidence of investment performance.

## Desktop installation and development

Download a macOS or Windows build from [Releases](https://github.com/roshameow/quantnight-frontend/releases/latest), then follow the [configuration guide](docs/user-guide.en.md).

```bash
npm ci
npm run tauri:dev
```

Building the desktop app also requires Rust and the [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/).

## Documentation

- [User guide](docs/user-guide.en.md): setup, tasks and results.
- [Architecture](docs/architecture.en.md): database routing, local/remote semantics and script execution.
- [Changelog](CHANGELOG.md): source and installer version notes.
- [Report an issue](https://github.com/roshameow/quantnight-frontend/issues): include your version, OS, reproduction steps and redacted logs.

## Version and license

Source version: **1.2.4**. Available binaries are listed in Releases. Historical `v1.2.3` assets used `0.1.0` filenames; see the changelog.

[MIT](LICENSE)
