{ inputs, ... }: {
  perSystem = { system, ... }: {
    phenixWrapped = {
      tend = inputs.phenix-tend.packages.${system}.tend;
    };
  };
}
