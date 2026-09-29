# NixOS module for the Invitro.ru appointment notifier.
#
# Usage in configuration.nix:
#
#   services.invitroBot = {
#     enable = true;
#     package = inputs.invitro-notification-bot.packages.${pkgs.system}.default;
#     environmentFile = "/run/secrets/invitro-bot.env"; # INVITRO_TELEGRAM_BOT_TOKEN, INVITRO_TELEGRAM_CHAT_ID
#   };
#
{ config, lib, pkgs, ... }:

let
  cfg = config.services.invitroBot;
in
{
  options.services.invitroBot = {
    enable = lib.mkEnableOption "Invitro.ru doctor appointment Telegram notifier";

    package = lib.mkOption {
      type = lib.types.package;
      description = "The invitro-bot package to run.";
    };

    environmentFile = lib.mkOption {
      type = lib.types.nullOr lib.types.path;
      default = null;
      description = ''
        Path to an EnvironmentFile with secrets, at least:
          INVITRO_TELEGRAM_BOT_TOKEN=<token from @BotFather>
          INVITRO_TELEGRAM_CHAT_ID=<chat id to notify>
      '';
    };

    doctorBitrixId = lib.mkOption {
      type = lib.types.int;
      default = 19143;
      description = "Numeric doctor id from the Invitro.ru page URL.";
    };

    citySlug = lib.mkOption {
      type = lib.types.str;
      default = "kurgan";
      description = "City slug from the page URL.";
    };

    specialtySlug = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = null;
      description = "Specialty slug to watch; defaults to the doctor's primary specialty.";
    };

    serviceIds = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [ ];
      description = ''
        Service UUIDs to watch. If empty, the doctor's main service at each
        office (the one used by the site's booking flow) is watched.
      '';
    };

    pollIntervalSecs = lib.mkOption {
      type = lib.types.int;
      default = 120;
      description = "Seconds between polls of the Invitro.ru API.";
    };

    notifyOnFirstRun = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = "Notify about slots that are already open on the very first poll.";
    };

    extraEnvironment = lib.mkOption {
      type = lib.types.attrsOf lib.types.str;
      default = { };
      description = "Additional INVITRO_* environment variables.";
    };
  };

  config = lib.mkIf cfg.enable {
    systemd.services.invitro-bot = {
      description = "Invitro.ru doctor slot notifier";
      after = [ "network-online.target" ];
      wants = [ "network-online.target" ];
      wantedBy = [ "multi-user.target" ];

      serviceConfig = {
        ExecStart = "${cfg.package}/bin/invitro-bot";
        Restart = "always";
        RestartSec = "10s";

        Environment = [
          "INVITRO_DOCTOR_BITRIX_ID=${toString cfg.doctorBitrixId}"
          "INVITRO_CITY_SLUG=${cfg.citySlug}"
          "INVITRO_POLL_INTERVAL_SECS=${toString cfg.pollIntervalSecs}"
          "INVITRO_STATE_FILE=/var/lib/invitro-bot/state.json"
          "INVITRO_NOTIFY_ON_FIRST_RUN=${if cfg.notifyOnFirstRun then "1" else "0"}"
        ] ++ lib.optional (cfg.specialtySlug != null) "INVITRO_SPECIALTY_SLUG=${cfg.specialtySlug}"
          ++ lib.optional (cfg.serviceIds != []) "INVITRO_SERVICE_IDS=${lib.concatStringsSep "," cfg.serviceIds}"
          ++ lib.mapAttrsToList (name: value: "${name}=${value}") cfg.extraEnvironment;

        EnvironmentFile = lib.optional (cfg.environmentFile != null) cfg.environmentFile;

        StateDirectory = "invitro-bot";
        DynamicUser = true;

        # Hardening: the bot only needs outbound HTTPS.
        NoNewPrivileges = true;
        PrivateTmp = true;
        PrivateDevices = true;
        ProtectSystem = "strict";
        ProtectHome = true;
        ProtectKernelTunables = true;
        ProtectKernelModules = true;
        ProtectControlGroups = true;
        RestrictAddressFamilies = [ "AF_INET" "AF_INET6" ];
        RestrictNamespaces = true;
        LockPersonality = true;
        MemoryDenyWriteExecute = true;
      };
    };
  };
}
