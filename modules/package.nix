{ inputs, lib, ... }: {
  perSystem =
    {
      config,
      pkgs,
      system,
      ...
    }:
    let
      filteredSrc = lib.cleanSource ../.;

      rustToolchain = [
        pkgs.cargo
        pkgs.rustc
        # These provide cargo-fmt and cargo-clippy subcommands for Tend.
        pkgs.rustfmt
        pkgs.clippy
      ];

      tendCliPkg = pkgs.rustPlatform.buildRustPackage {
        pname = "tend";
        version = "0.1.0";
        src = filteredSrc;
        cargoLock.lockFile = ../Cargo.lock;
        cargoBuildFlags = "-p tend-cli";
        nativeBuildInputs = [ pkgs.git ];
      };

      tendMcpPkg = pkgs.rustPlatform.buildRustPackage {
        pname = "tend-mcp";
        version = "0.1.0";
        src = filteredSrc;
        cargoLock.lockFile = ../Cargo.lock;
        cargoBuildFlags = "-p tend-mcp";
        nativeBuildInputs = [ pkgs.git ];
      };

      # Reuse vendored crate dependencies from any buildRustPackage.
      cargoDeps = tendCliPkg.cargoDeps or (throw "cargoDeps not found");

      mkCargoCheck =
        name: description: cargoArgs: extraNativeBuildInputs:
        pkgs.runCommand name
          {
            nativeBuildInputs = extraNativeBuildInputs ++ [ pkgs.stdenv.cc ];
            inherit cargoDeps;
            src = filteredSrc;
          }
          ''
            export HOME=$TMPDIR/home
            mkdir -p $HOME
            export CARGO_HOME=$TMPDIR/cargo
            export CARGO_TARGET_DIR=$TMPDIR/target
            mkdir -p $CARGO_HOME $CARGO_TARGET_DIR

            cp -rT $src source
            chmod -R u+w source
            cd source

            # Point cargo at the vendored dependencies
            mkdir -p .cargo
            cat > .cargo/config.toml <<EOF
            [source.crates-io]
            replace-with = "vendored-sources"

            [source.vendored-sources]
            directory = "${cargoDeps}"
            EOF

            ${cargoArgs}

            touch $out
          '';
    in
    {
      packages = {
        inherit
          tendCliPkg
          tendMcpPkg
          ;
        tend = tendCliPkg;
        tend-mcp = tendMcpPkg;
        default = tendCliPkg;
      };

      checks = {
        cargo-check =
          mkCargoCheck "phenix-tend-cargo-check" "cargo check --workspace --all-targets"
            "cargo check --workspace --all-targets"
            rustToolchain;

        cargo-test =
          mkCargoCheck "phenix-tend-cargo-test" "cargo test --workspace" "cargo test --workspace"
            [
              pkgs.cargo
              pkgs.rustc
              pkgs.git
            ];

        cargo-fmt =
          mkCargoCheck "phenix-tend-cargo-fmt" "cargo fmt --all --check" "cargo fmt --all --check"
            rustToolchain;

        cargo-clippy =
          mkCargoCheck "phenix-tend-cargo-clippy"
            "cargo clippy --quiet --workspace --all-targets -- -D warnings"
            "cargo clippy --quiet --workspace --all-targets -- -D warnings"
            rustToolchain;

        tend-gate =
          pkgs.runCommand "phenix-tend-tend-gate"
            {
              nativeBuildInputs = [
                tendCliPkg
                pkgs.git
                pkgs.nix
                pkgs.nixfmt
                pkgs.statix
                pkgs.deadnix
                pkgs.stdenv.cc
              ]
              ++ rustToolchain;
              inherit cargoDeps;
              src = filteredSrc;
            }
            ''
              export HOME=$TMPDIR/home
              mkdir -p $HOME
              export NIX_STATE_DIR=$TMPDIR/nix-state
              mkdir -p $NIX_STATE_DIR/profiles
              export NIX_PATH=nixpkgs=${pkgs.path}
              export CARGO_HOME=$TMPDIR/cargo
              export CARGO_TARGET_DIR=$TMPDIR/target
              mkdir -p $CARGO_HOME $CARGO_TARGET_DIR

              cp -rT $src source
              chmod -R u+w source
              cd source

              # Point cargo at the vendored dependencies
              mkdir -p .cargo
              cat > .cargo/config.toml <<EOF
              [source.crates-io]
              replace-with = "vendored-sources"

              [source.vendored-sources]
              directory = "${cargoDeps}"
              EOF

              # git is needed by tend for changed-file detection
              git init && git add -A

              tend run --mode full --phase verify --profile nix-check

              touch $out
            '';
      };

      apps = {
        tend = {
          type = "app";
          program = "${tendCliPkg}/bin/tend";
        };
        tend-mcp = {
          type = "app";
          program = "${tendMcpPkg}/bin/tend-mcp";
        };
        default = {
          type = "app";
          program = "${tendCliPkg}/bin/tend";
        };
      };

      devShells.default = pkgs.mkShell {
        name = "phenix-tend-dev";
        packages = [
          pkgs.rust-analyzer
          pkgs.git
          pkgs.nix
          tendCliPkg
        ]
        ++ rustToolchain;
        shellHook = ''
          echo "phenix-tend dev shell"
          echo "  cargo: $(cargo --version 2>/dev/null || echo '?')"
          echo "  rustc: $(rustc --version 2>/dev/null || echo '?')"
          echo "  tend:  $(tend --version 2>/dev/null || echo '?')"
        '';
      };
    };
}
