# NixOS module for task-server
#
# Usage in your NixOS configuration:
#
#   # flake.nix inputs:
#   inputs.task.url = "github:FastTrackStudios/task";
#
#   # configuration.nix:
#   imports = [ inputs.task.nixosModules.task-server ];
#
#   services.task-server = {
#     enable = true;
#     vaultRoot = "/mnt/starcommand/Projects";
#     serverName = "starcommand";
#     port = 3456;
#     nextcloud = {
#       enable = true;
#       url = "https://cloud.starcommand.live";
#       username = "codywright";
#       passwordFile = config.sops.secrets."task/nextcloud-password".path;
#       projectsPath = "Projects/";
#       calendar = "Personal";
#       deckEnabled = true;
#     };
#   };
#
{ config, lib, pkgs, ... }:

let
  cfg = config.services.task-server;
  inherit (lib) mkEnableOption mkOption mkIf types;
in
{
  options.services.task-server = {
    enable = mkEnableOption "Task management server";

    package = mkOption {
      type = types.package;
      description = "The task-server package to use.";
    };

    # ── Server identity ───────────────────────────────────────────────

    serverName = mkOption {
      type = types.str;
      default = config.networking.hostName;
      description = "Human-readable name for this server instance.";
    };

    serverId = mkOption {
      type = types.str;
      default = "${config.networking.hostName}-task";
      description = "Unique stable identifier for this server.";
    };

    # ── Network ───────────────────────────────────────────────────────

    port = mkOption {
      type = types.port;
      default = 3456;
      description = "Port to listen on.";
    };

    bindAddress = mkOption {
      type = types.str;
      default = "0.0.0.0";
      description = "Address to bind to.";
    };

    openFirewall = mkOption {
      type = types.bool;
      default = false;
      description = "Whether to open the port in the firewall.";
    };

    # ── Vault configuration ───────────────────────────────────────────

    vaultRoot = mkOption {
      type = types.path;
      description = ''
        Root directory for projects. This is where the server reads
        project.md + tasks/*.md files.

        Can be a local path, NFS mount, or Nextcloud sync directory.
      '';
    };

    databasePath = mkOption {
      type = types.path;
      default = "/var/lib/task-server/task.sqlite";
      description = "SQLite database path for auth, organizations, activity, and other authoritative service data.";
    };

    publicBaseUrl = mkOption {
      type = types.nullOr types.str;
      default = null;
      description = "Public base URL used by auth callbacks. Defaults to the bind address.";
    };

    seedDemo = mkOption {
      type = types.bool;
      default = true;
      description = "Seed demo users, organizations, projects, and tasks on startup. Disable for production instances.";
    };

    # ── Nextcloud integration ─────────────────────────────────────────

    nextcloud = {
      enable = mkEnableOption "Nextcloud integration";

      url = mkOption {
        type = types.str;
        default = "";
        description = "Nextcloud base URL (e.g. https://cloud.example.com).";
      };

      username = mkOption {
        type = types.str;
        default = "";
        description = "Nextcloud username.";
      };

      passwordFile = mkOption {
        type = types.nullOr types.path;
        default = null;
        description = ''
          Path to a file containing the Nextcloud app password.
          The file should contain just the password, no newline.
          Use with sops-nix or agenix for secrets management.
        '';
      };

      projectsPath = mkOption {
        type = types.str;
        default = "Projects/";
        description = "Path within the user's Nextcloud files to the Projects directory.";
      };

      calendar = mkOption {
        type = types.str;
        default = "Personal";
        description = "CalDAV calendar name for task sync.";
      };

      deckEnabled = mkOption {
        type = types.bool;
        default = false;
        description = "Whether to sync with Nextcloud Deck boards.";
      };
    };

    # ── CalDAV sync ───────────────────────────────────────────────────

    caldav = {
      enable = mkEnableOption "CalDAV task sync (VTODO)";

      serverUrl = mkOption {
        type = types.str;
        default = "";
        description = "CalDAV server URL.";
      };

      username = mkOption {
        type = types.str;
        default = "";
        description = "CalDAV username.";
      };

      passwordFile = mkOption {
        type = types.nullOr types.path;
        default = null;
        description = "Path to file containing CalDAV password.";
      };

      calendarPath = mkOption {
        type = types.str;
        default = "calendars/user/tasks/";
        description = "Calendar collection path on the CalDAV server.";
      };

      cacheDir = mkOption {
        type = types.path;
        default = "/var/lib/task-server/caldav-cache";
        description = "Local cache directory for .ics files.";
      };
    };

    # ── Logging ───────────────────────────────────────────────────────

    logLevel = mkOption {
      type = types.str;
      default = "info";
      description = "Log level (trace, debug, info, warn, error).";
    };

    # ── User/group ────────────────────────────────────────────────────

    user = mkOption {
      type = types.str;
      default = "task-server";
      description = "User to run the server as.";
    };

    group = mkOption {
      type = types.str;
      default = "task-server";
      description = "Group to run the server as.";
    };
  };

  config = mkIf cfg.enable {
    # Create system user and group
    users.users.${cfg.user} = {
      isSystemUser = true;
      group = cfg.group;
      home = "/var/lib/task-server";
      createHome = true;
    };

    users.groups.${cfg.group} = {};

    # Firewall
    networking.firewall.allowedTCPPorts = mkIf cfg.openFirewall [ cfg.port ];

    # Systemd service
    systemd.services.task-server = {
      description = "Task Management Server";
      after = [ "network-online.target" ];
      wants = [ "network-online.target" ];
      wantedBy = [ "multi-user.target" ];

      environment = {
        TASK_VAULT = toString cfg.vaultRoot;
        VAULT_ROOT = toString cfg.vaultRoot;
        TASK_DB_PATH = toString cfg.databasePath;
        TASK_SEED_DEMO = if cfg.seedDemo then "1" else "0";
        SERVER_NAME = cfg.serverName;
        SERVER_ID = cfg.serverId;
        BIND_ADDR = "${cfg.bindAddress}:${toString cfg.port}";
        RUST_LOG = "task_server=${cfg.logLevel}";
      } // lib.optionalAttrs (cfg.publicBaseUrl != null) {
        PUBLIC_BASE_URL = cfg.publicBaseUrl;
      } // lib.optionalAttrs cfg.nextcloud.enable {
        NEXTCLOUD_URL = cfg.nextcloud.url;
        NEXTCLOUD_USERNAME = cfg.nextcloud.username;
        NEXTCLOUD_PROJECTS_PATH = cfg.nextcloud.projectsPath;
        NEXTCLOUD_CALENDAR = cfg.nextcloud.calendar;
        NEXTCLOUD_DECK_ENABLED = if cfg.nextcloud.deckEnabled then "1" else "0";
      } // lib.optionalAttrs cfg.caldav.enable {
        CALDAV_URL = cfg.caldav.serverUrl;
        CALDAV_USERNAME = cfg.caldav.username;
        CALDAV_CALENDAR_PATH = cfg.caldav.calendarPath;
        CALDAV_CACHE_DIR = cfg.caldav.cacheDir;
      };

      serviceConfig = {
        Type = "simple";
        ExecStart = "${cfg.package}/bin/task-server";
        User = cfg.user;
        Group = cfg.group;
        Restart = "on-failure";
        RestartSec = "5s";

        # Security hardening
        NoNewPrivileges = true;
        ProtectSystem = "strict";
        ProtectHome = "read-only";
        PrivateTmp = true;
        ReadWritePaths = [
          (toString cfg.vaultRoot)
          "/var/lib/task-server"
          (toString (dirOf cfg.databasePath))
        ] ++ lib.optionals cfg.caldav.enable [
          cfg.caldav.cacheDir
        ];

        # Load secrets from files
        LoadCredential = lib.optional (cfg.nextcloud.enable && cfg.nextcloud.passwordFile != null)
          "nextcloud-password:${cfg.nextcloud.passwordFile}"
        ++ lib.optional (cfg.caldav.enable && cfg.caldav.passwordFile != null)
          "caldav-password:${cfg.caldav.passwordFile}";
      };

      # Read password from credential files into env vars
      preStart = let
        ncPw = lib.optionalString (cfg.nextcloud.enable && cfg.nextcloud.passwordFile != null) ''
          export NEXTCLOUD_PASSWORD="$(cat $CREDENTIALS_DIRECTORY/nextcloud-password)"
        '';
        cdPw = lib.optionalString (cfg.caldav.enable && cfg.caldav.passwordFile != null) ''
          export CALDAV_PASSWORD="$(cat $CREDENTIALS_DIRECTORY/caldav-password)"
        '';
      in ncPw + cdPw;
    };

    # Create cache directory
    systemd.tmpfiles.rules = [
      "d ${dirOf cfg.databasePath} 0750 ${cfg.user} ${cfg.group} -"
    ] ++ lib.optionals cfg.caldav.enable [
      "d ${cfg.caldav.cacheDir} 0750 ${cfg.user} ${cfg.group} -"
    ];
  };
}
