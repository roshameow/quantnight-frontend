## First-Time Use and Configuration

Before you begin, please ensure your environment is configured correctly.

### 1. Environment Setup

- **MongoDB Database**: The application needs to connect to a running MongoDB instance. Please ensure the following three databases have been created in the instance:
  1.  `simulation_mission`
  2.  `simulation_db`
  3.  `alpha_db`
- **Create `tasks` Collection**: In the `simulation_mission` database, you **must** manually create an empty collection named `tasks`, otherwise the application will not start correctly.

### 2. Configuration Management

The application TOML selects `env.working_dir`, UI database/collection names and
scripts. Connection endpoints remain authoritative in that backend's original
MongoConfig (private `credentials.yaml`), not legacy TOML URI fields. Settings
does not replace backend credential/endpoint configuration. Never upload credentials
or task receipts.

**After explicit macOS runtime enrollment**, GUI and CLI are equal clients of one
headless task host. A user-level launchd socket starts it on first demand, without
an open GUI or MCP. Task lists/remote-only cache, diagnostics, operations and
progress projections belong to that host. Non-task Alpha/data/PNL/correlation
features retain the original independent GUI data pool.

- Follow the [single build/deployment guide](../crates/taskctl/README.md#runtime-enrollment-and-safe-adoption)
  and [CLI guide](cli-usage.md). Dragging an app into Applications or editing disk
  files does not enroll or load a task host.
- Closing the GUI does not stop the host or runner, or cancel accepted platform
  requests. Reopening reconnects a client. A lost operation response never permits
  automatic resend; reconcile original task/generation/launch/business receipts.
- UI settings keep their original application rules; reopen the GUI for its
  non-task data configuration. **Restarting only the GUI does not adopt task-host
  configuration/shared-source changes**. The original deployment owner drains
  accepted work, explicitly adopts the new host and verifies its loaded identity;
  do not kill a busy host.
- GUI task diagnostics require the explicitly selected canonical original AppConfig
  so the host retains its check_task_status script contract. The original loader
  still uses MongoConfig endpoint authority. The deployment candidate flag is
  `--runtime-app-config` (see the single guide), not an AppConfig/URI upload or
  arbitrary script RPC. backend_root keeps pure CLI semantics. Executable-only
  schema 2 upgrades retain the selector; stop the old binding safely before
  publishing any selector change.
- Schema 1 without runtime enrollment retains the original mode. First-version
  runtime enrollment is macOS-only; original unenrolled Windows/other paths remain.
  This is the source contract, not proof that a downloaded release has adopted it.

---

## Usage Guide

### Task Manager

This is the main interface where you can see the list of all created tasks.

- **View tasks**: The list shows task name, type, status, etc.
- **Operate tasks**: Each task card provides `Start` / `Pause` / `Delete` action buttons.
- **Task card progress**: Cards show `✓ success | ✗ failed | Σ total` (format: `normal+priority`). Progress is pushed by the watcher in real time; for isRemote tasks it shows the **remote DB** status.

### Creating New Tasks

Click the `+ Add Task` button in the top right corner to create one of three task types:

- **Add Task**: Creates a regular quantitative task.
- **Add Super Task**: Creates a super task for more complex combinations or workflows.
- **Add Priority Task**: Creates a high-priority task.

In the dialog, enter a task name and select a template file. On start, you can set task parameters in the "Config JSON" (e.g. the `adaptive` adaptive-scheduling config).

### Data Analysis

Switch to the `Data` page to analyze results in depth.

- **Select result set**: Choose a data collection from the dropdown (e.g. `alpha_results`).
- **Use filters**: Filter data with the filter boxes (ID, Region, Turnover, etc.).
- **Compute correlation**: After filtering, click `Compute Correlation` to run correlation analysis.

### Settings

Click the gear icon `⚙️` to open the settings panel:

- **Frontend config**:
  - **Template Paths**: Set the folder paths for template files used to create different task types.
  - **Data Filter Options**: Customize the result-set dropdown options on the Data analysis page.

- **Backend config**:
  - **MongoDB Settings**: Keep UI database/collection settings; endpoints come from original backend MongoConfig, not legacy TOML URI overrides. Task namespaces must match enrollment; non-task Alpha/data/PNL settings remain independent.
  - **Python Settings**: Set the Python interpreter path and working directory.
  - **Script Mapping**: Manage button-to-script mappings, with support for custom scripts.

> **Note**: The GUI non-task pool and task host load configuration separately. Adopt task endpoint/shared-source changes safely through the guide above; reopening only the GUI does not replace the task host.

---

## Installation

These steps install the desktop package, not runtime enrollment/adoption. Follow
the guide above for explicit macOS task-host adoption; desktop installation alone
does not prove launchd registration.

### macOS

1.  **Download**: Get the latest `.dmg` from the Releases page.
2.  **Install**: Open the `.dmg` and drag `QuantNight.app` into `Applications`.
3.  **Security notice**: If macOS says the app is damaged or can't be opened, run:
    ```bash
    sudo xattr -r -d com.apple.quarantine /Applications/QuantNight.app
    ```

### Windows

1.  **Download**: Get the latest `.msi` installer from the Releases page.
2.  **Install**: Run the `.msi` and follow the wizard.
3.  **Security notice**: If Windows SmartScreen blocks it, click `More info` -> `Run anyway`.
