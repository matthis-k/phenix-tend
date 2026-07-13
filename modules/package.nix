{ lib, ... }: {
  perSystem =
    { pkgs, ... }:
    let
      source = lib.cleanSource ../.;

      rustToolchain = [
        pkgs.cargo
        pkgs.rustc
        pkgs.rustfmt
        pkgs.clippy
      ];

      tendRuntime = [
        pkgs.bash
        pkgs.findutils
        pkgs.git
        pkgs.nix
        pkgs.nixfmt
        pkgs.statix
        pkgs.deadnix
      ]
      ++ rustToolchain;

      tendCliPkg = pkgs.rustPlatform.buildRustPackage {
        pname = "tend";
        version = "0.1.0";
        src = source;
        cargoLock.lockFile = ../Cargo.lock;
        cargoBuildFlags = "-p tend-cli";
        nativeBuildInputs = [ pkgs.git ];
      };

      tendRunner = pkgs.writeShellApplication {
        name = "tend";
        runtimeInputs = tendRuntime;
        text = ''
          exec ${tendCliPkg}/bin/tend "$@"
        '';
      };

      tendRustfmtCheck = pkgs.writeShellApplication {
        name = "tend-rustfmt-check";
        runtimeInputs = [
          pkgs.findutils
          pkgs.rustfmt
        ];
        text = ''
          files=("$@")
          if (( ''${#files[@]} == 0 )); then
            mapfile -d $'\0' files < <(
              find crates -name '*.rs' -type f -print0 | sort -z
            )
          fi

          if (( ''${#files[@]} > 0 )); then
            exec rustfmt --edition 2021 --check "''${files[@]}"
          fi
        '';
      };

      tendRustfmtFix = pkgs.writeShellApplication {
        name = "tend-rustfmt-fix";
        runtimeInputs = [
          pkgs.findutils
          pkgs.rustfmt
        ];
        text = ''
          files=("$@")
          if (( ''${#files[@]} == 0 )); then
            mapfile -d $'\0' files < <(
              find crates -name '*.rs' -type f -print0 | sort -z
            )
          fi

          if (( ''${#files[@]} > 0 )); then
            exec rustfmt --edition 2021 "''${files[@]}"
          fi
        '';
      };

      tendNixfmtCheck = pkgs.writeShellApplication {
        name = "tend-nixfmt-check";
        runtimeInputs = [
          pkgs.findutils
          pkgs.nixfmt
        ];
        text = ''
          files=("$@")
          if (( ''${#files[@]} == 0 )); then
            mapfile -d $'\0' files < <(
              find . \
                -path './.git' -prune -o \
                -path './target' -prune -o \
                -name '*.nix' -type f -print0 |
                sort -z
            )
          fi

          if (( ''${#files[@]} > 0 )); then
            exec nixfmt --check "''${files[@]}"
          fi
        '';
      };

      tendNixfmtFix = pkgs.writeShellApplication {
        name = "tend-nixfmt-fix";
        runtimeInputs = [
          pkgs.findutils
          pkgs.nixfmt
        ];
        text = ''
          files=("$@")
          if (( ''${#files[@]} == 0 )); then
            mapfile -d $'\0' files < <(
              find . \
                -path './.git' -prune -o \
                -path './target' -prune -o \
                -name '*.nix' -type f -print0 |
                sort -z
            )
          fi

          if (( ''${#files[@]} > 0 )); then
            exec nixfmt "''${files[@]}"
          fi
        '';
      };

      tendStatixFix = pkgs.writeShellApplication {
        name = "tend-statix-fix";
        runtimeInputs = [ pkgs.statix ];
        text = ''
          files=("$@")
          if (( ''${#files[@]} == 0 )); then
            exec statix fix
          fi

          for file in "''${files[@]}"; do
            statix fix "$file"
          done
        '';
      };

      lifecycleCommands = [
        tendRustfmtCheck
        tendRustfmtFix
        tendNixfmtCheck
        tendNixfmtFix
        tendStatixFix
      ];

      cargoDeps = tendCliPkg.cargoDeps or (throw "cargoDeps not found");

      tendGate =
        pkgs.runCommand "phenix-tend-gate"
          {
            nativeBuildInputs = tendRuntime ++ lifecycleCommands ++ [ pkgs.stdenv.cc ];
            inherit cargoDeps;
            src = source;
          }
          ''
            export HOME=$TMPDIR/home
            mkdir -p "$HOME"
            export NIX_STATE_DIR=$TMPDIR/nix-state
            mkdir -p "$NIX_STATE_DIR/profiles"
            export NIX_PATH=nixpkgs=${pkgs.path}
            export CARGO_HOME=$TMPDIR/cargo
            export CARGO_TARGET_DIR=$TMPDIR/target
            mkdir -p "$CARGO_HOME" "$CARGO_TARGET_DIR"

            cp -rT "$src" source
            chmod -R u+w source
            cd source

            mkdir -p .cargo
            cat > .cargo/config.toml <<EOF
            [source.crates-io]
            replace-with = "vendored-sources"

            [source.vendored-sources]
            directory = "${cargoDeps}"
            EOF

            git init --quiet
            git add -A

            ${tendCliPkg}/bin/tend check --profile full --context nix-sandbox

            touch "$out"
          '';

      tendFix = pkgs.writeShellApplication {
        name = "tend-fix";
        runtimeInputs = [
          tendRunner
          pkgs.git
        ]
        ++ lifecycleCommands;
        text = ''
          repo_root="$(git rev-parse --show-toplevel)"
          cd "$repo_root"

          mapfile -d $'\0' staged_files < <(
            git diff --cached --name-only --diff-filter=ACMR -z
          )

          partially_staged=()
          for file in "''${staged_files[@]}"; do
            [[ -e "$file" ]] || continue
            if ! git diff --quiet -- "$file"; then
              partially_staged+=("$file")
            fi
          done

          if (( ''${#partially_staged[@]} > 0 )); then
            printf '%s\n' \
              'Cannot apply staged repairs to partially staged files.' \
              'Stage or stash their remaining changes first:' >&2
            printf '  %s\n' "''${partially_staged[@]}" >&2
            exit 1
          fi

          tend check --profile fix --context local

          if (( ''${#staged_files[@]} > 0 )); then
            git add -- "''${staged_files[@]}"
          fi

          exec tend check --profile git-hook --context local
        '';
      };

      tendVerify = pkgs.writeShellApplication {
        name = "tend-verify";
        runtimeInputs = [ tendRunner ] ++ lifecycleCommands;
        text = ''
          exec tend check --profile manual --context local "$@"
        '';
      };

      tendPrePush = pkgs.writeShellApplication {
        name = "tend-pre-push";
        runtimeInputs = [ tendRunner ];
        text = ''
          exec tend check --profile pre-push --context local "$@"
        '';
      };

      gitHooks = pkgs.runCommand "phenix-tend-git-hooks" { } ''
        mkdir -p "$out"

        cat > "$out/pre-commit" <<'EOF'
        #!/usr/bin/env bash
        set -euo pipefail
        repo_root="$(${pkgs.git}/bin/git rev-parse --show-toplevel)"
        exec ${pkgs.nix}/bin/nix develop "$repo_root" --command tend-fix
        EOF

        cat > "$out/pre-push" <<'EOF'
        #!/usr/bin/env bash
        set -euo pipefail
        repo_root="$(${pkgs.git}/bin/git rev-parse --show-toplevel)"
        exec ${pkgs.nix}/bin/nix develop "$repo_root" --command tend-pre-push
        EOF

        chmod +x "$out/pre-commit" "$out/pre-push"
      '';
    in
    {
      packages = {
        tend = tendRunner;
        tend-unwrapped = tendCliPkg;
        default = tendRunner;
      };

      checks = {
        tend-package = tendRunner;
        tend-gate = tendGate;
      };

      apps = {
        tend = {
          type = "app";
          program = "${tendRunner}/bin/tend";
          meta.description = "Select and execute repository-local quality tasks";
        };
        default = {
          type = "app";
          program = "${tendRunner}/bin/tend";
          meta.description = "Select and execute repository-local quality tasks";
        };
      };

      devShells.default = pkgs.mkShell {
        name = "phenix-tend-dev";
        packages = [
          tendRunner
          tendFix
          tendVerify
          tendPrePush
          pkgs.rust-analyzer
        ]
        ++ lifecycleCommands
        ++ tendRuntime;
        shellHook = ''
          if repo_root="$(git rev-parse --show-toplevel 2>/dev/null)"; then
            git -C "$repo_root" config --local core.hooksPath ${gitHooks}
            hooks_status="enabled"
          else
            hooks_status="not in a Git repository"
          fi

          echo "phenix-tend dev shell"
          echo "  hooks:   $hooks_status"
          echo "  fix:     tend-fix"
          echo "  verify:  tend-verify"
          echo "  prepush: tend-pre-push"
          echo "  tend:    $(tend --version 2>/dev/null || echo '?')"
        '';
      };
    };
}
