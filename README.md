# FastPRReviewer

A tool for automatically reviewing and approving pull requests in Azure DevOps. This utility monitors PRs from specific users and can automatically approve them, saving time and streamlining your team's workflow.

## Setup Guide

### 1. Configuration Setup

This project uses a TOML configuration file to store your Azure DevOps settings:

1. Create your configuration file by copying the template:
   ```bash
   # On Windows
   copy config.template.toml config.toml
   
   # On Linux/Mac
   cp config.template.toml config.toml
   ```

2. Edit `config.toml` with your organization-specific information:
   ```toml
   organization = "your-organization-name"
   project = "your-project-name"
   personal_access_token = "your-pat-here"
   watched_users = ["Full Name 1", "Full Name 2"]
   ```

### 2. Creating an Azure DevOps Personal Access Token (PAT)

1. Navigate to your Azure DevOps organization settings:
   - Go to `https://dev.azure.com/{your-organization}/_usersSettings/tokens`
   - Or click on your profile picture → Personal Access Tokens

2. Click "New Token" and configure as follows:
   - Name: `FastPRReviewer` (or any descriptive name)
   - Organization: Select your organization
   - Expiration: Choose an appropriate expiration date
   - Scopes: Select "Custom defined"
   - Permissions required:
     - Code: Read & Write

3. Click "Create" and copy the generated token to your `config.toml` file

### 3. Configuring Watched Users

In the `watched_users` array, specify the **full names** of users whose PRs should be automatically approved:

```toml
watched_users = [
  "John Doe",
  "Jane Smith"
]
```

Note: The names must match exactly how they appear in Azure DevOps.

### 4. Running the Program

To start monitoring and automatically approving PRs created AFTER the program started from the watched users:

```bash
cargo run
```

The program will check for new PRs from watched users at regular intervals and automatically approve them when found.

## Advanced Usage

To run the program in the background or as a service, consider using:
- Windows: Task Scheduler
- Linux: Systemd service or Cron job
- Docker: See the Docker setup instructions below (if applicable)

## Troubleshooting

- If you encounter authentication errors, verify your PAT has not expired and has the correct permissions
- Ensure the full names in `watched_users` match exactly with Azure DevOps user names

Note: The `config.toml` file is excluded from Git to prevent accidental commitment of credentials. Only the template version is tracked.
