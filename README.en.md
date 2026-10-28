# QuantNight

[中文](README.md) · [Download](https://github.com/roshameow/quantnight-frontend/releases/latest) · [User guide](docs/user-guide.en.md)

A desktop console for quantitative research: manage regular, Priority and SUPER tasks, track local and remote execution, and explore results.

![QuantNight task manager demo](assets/demo.gif)

## Try the interface

Requires Node.js 22 or later. The interactive demo uses synthetic data and needs no database, platform account or backend:

```bash
git clone https://github.com/roshameow/quantnight-frontend.git
cd quantnight-frontend
npm ci
npm run demo
```

Open `/demo.html` at the address printed in the terminal. Browse task cards, toggle local/remote labels and filter sample results. Start, pause and delete change in-memory demo data only; refreshing resets it. No real simulations are started.

## Scope

- **Standalone demo**: browse and interact with sample tasks without other services.
- **Desktop application**: manage tasks, configure execution parameters and explore results stored in your database.
- **Real task execution**: requires a compatible private `quantnight` backend, MongoDB, script mappings and your own platform account. The backend is not included in this repository or the desktop download.

Demo results are fictional, not actual backtest performance.

## Desktop installation and configuration

Download an installer for your system from [Releases](https://github.com/roshameow/quantnight-frontend/releases/latest). If you have a compatible backend, follow the [user guide](docs/user-guide.en.md) for database setup, task management and data views.

Connection setup for the current source:

1. Set `env.working_dir` in the application TOML to your backend root, and configure database/collection names and script mappings. See the [bundled configuration template](src-tauri/resources/config.toml).
2. Configure `mongodb.local_uri`, `mongodb.remote_uri` and your platform account in that backend's private `credentials.yaml`. Desktop connections use the backend configuration, not the legacy URI fields in TOML. Both database roles must be configured, and task database names must match the backend.
3. Close and reopen the application after connection changes. The template picker selects existing backend templates; it does not upload files. Place templates in the relevant execution environment first.

Once connected, add a task, select its template and configure its parameters. Use task cards to start or pause execution and the data page to filter results. Pausing local execution does not cancel simulations already accepted by the platform.

## Source development

Requires Node.js 22+, Rust and the [Tauri system prerequisites](https://v2.tauri.app/start/prerequisites/). Desktop development also needs a compatible backend and the configuration above. First register your frontend/backend paths using the [CLI development setup guide](crates/taskctl/README.md#explicit-desktop-development-taskctl-mode), then run:

```bash
npm ci
npm run tauri:dev
```

To try only the interface, use `npm run demo`; desktop development registration is not required.

## Documentation and feedback

- [User guide](docs/user-guide.en.md): initial setup, tasks and data analysis.
- [Developer architecture](docs/architecture.en.md): data flow, local/remote execution and script interfaces.
- [Command-line tool](crates/taskctl/README.md): building, configuration and usage.
- [Changelog](CHANGELOG.md) · [Report an issue](https://github.com/roshameow/quantnight-frontend/issues) with your version, OS, reproduction steps and redacted logs.

## License

[MIT](LICENSE)
