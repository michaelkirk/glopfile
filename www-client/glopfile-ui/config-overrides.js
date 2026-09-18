const path = require("path");

module.exports = function override(config, env) {
  if (!config.experiments) {
    config.experiments = {};
  }

  config.experiments.asyncWebAssembly = true;

  config.module.rules.push({
    test: /\.wasm$/,
    type: 'webassembly/async',
  });

  if (!config.plugins) {
    config.plugins = [];
  }

  return config;
}
