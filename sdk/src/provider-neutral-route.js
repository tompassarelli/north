import { str as $$bc$str } from './bridge/generated/beagle/core.js';

const catalog_module = require("./providers/catalog");

const providerSupportsRoute = catalog_module.providerSupportsRoute;

function requireProviderNeutralRoute(capability_floor, service_class, reasoning) {
  if ((!(providerSupportsRoute("anthropic", capability_floor, service_class, reasoning) || providerSupportsRoute("openai", capability_floor, service_class, reasoning)))) {
    (() => { throw new Error($$bc$str("unsupported route: capability floor '", capability_floor, "' with service class '", service_class, "' and deliberation '", reasoning, "' resolves through no provider catalog")); })();
  }
  return null;
}

export { requireProviderNeutralRoute as "requireProviderNeutralRoute" };
