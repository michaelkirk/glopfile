const path = require("path");

module.exports = {
  webpack: function (config, env) {
    if (!config.experiments) {
      config.experiments = {};
    }

    config.experiments.asyncWebAssembly = true;

    config.module.rules.push({
      test: /\.wasm$/,
      type: "webassembly/async",
    });

    if (!config.plugins) {
      config.plugins = [];
    }

    return config;
  },

  // glopfile and its wasm glue ship as ES modules, which jest will not load
  // from node_modules without being told to, and the wasm itself it cannot
  // instantiate at all.
  jest: function (config) {
    config.transformIgnorePatterns = [
      "/node_modules/(?!(glopfile|glopfile-www-rust)/)",
    ];
    config.moduleNameMapper = {
      ...config.moduleNameMapper,
      "^glopfile-www-rust$": path.resolve(__dirname, "src/testing/wasmStub.js"),
      "\\.wasm$": path.resolve(__dirname, "src/testing/wasmStub.js"),
    };
    return config;
  },
};
