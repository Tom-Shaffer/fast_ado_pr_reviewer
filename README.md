# FastPRReviewer

A tool for automatically reviewing and approving pull requests in Azure DevOps.

## Configuration

This project uses a TOML configuration file to store your Azure DevOps settings:

1. Copy the template configuration file to create your own:
   ```
   cp config.template.toml config.toml
   ```

2. Edit `config.toml` with your organization-specific information:
   - `organization`: Your Azure DevOps organization name
   - `project`: Your Azure DevOps project name
   - `personal_access_token`: Your Azure DevOps PAT with Code (Read) and Pull Request (Read & Write) permissions
   - `watched_users`: List of users whose PRs will be automatically approved

Note: The `config.toml` file is excluded from Git to prevent accidental commitment of credentials. Only the template version is tracked.