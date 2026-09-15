## First-Time Use and Configuration

Before you begin, please ensure your environment is configured correctly.

### 1. Environment Setup

- **MongoDB Database**: The application needs to connect to a running MongoDB instance. Please ensure the following three databases have been created in the instance:
  1.  `simulation_mission`
  2.  `simulation_db`
  3.  `alpha_db`
- **Create `tasks` Collection**: In the `simulation_mission` database, you **must** manually create an empty collection named `tasks`, otherwise the application will not start correctly.

### 2. Configuration Management

The application provides a complete graphical configuration interface, so you don't need to edit any configuration files manually. On first launch, the application will automatically create the necessary configuration files.

- **All configurations can be done through the UI**:
  - Frontend config (template paths, data filter options, etc.)
  - Backend config (MongoDB connections, Python interpreter path, etc.)
  - Script mapping config

- **Config file locations**:
  - User data directory (preferred)
  - App resource directory (fallback)

> **Note**: Frontend config changes take effect immediately; backend config changes require an app restart.

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
  - **MongoDB Settings**: Configure local and remote connection strings (`local_uri` / `remote_uri`) and database names.
  - **Python Settings**: Set the Python interpreter path and working directory.
  - **Script Mapping**: Manage button-to-script mappings, with support for custom scripts.

> **Note**: Backend config changes require an app restart.

---

## Installation

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
