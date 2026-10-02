var __create = Object.create;
var __defProp = Object.defineProperty;
var __getOwnPropDesc = Object.getOwnPropertyDescriptor;
var __getOwnPropNames = Object.getOwnPropertyNames;
var __getProtoOf = Object.getPrototypeOf;
var __hasOwnProp = Object.prototype.hasOwnProperty;
var __esm = (fn, res, err) => function __init() {
  if (err) throw err[0];
  try {
    return fn && (res = (0, fn[__getOwnPropNames(fn)[0]])(fn = 0)), res;
  } catch (e) {
    throw err = [e], e;
  }
};
var __commonJS = (cb, mod) => function __require() {
  try {
    return mod || (0, cb[__getOwnPropNames(cb)[0]])((mod = { exports: {} }).exports, mod), mod.exports;
  } catch (e) {
    throw mod = 0, e;
  }
};
var __export = (target, all) => {
  for (var name in all)
    __defProp(target, name, { get: all[name], enumerable: true });
};
var __copyProps = (to, from, except, desc) => {
  if (from && typeof from === "object" || typeof from === "function") {
    for (let key of __getOwnPropNames(from))
      if (!__hasOwnProp.call(to, key) && key !== except)
        __defProp(to, key, { get: () => from[key], enumerable: !(desc = __getOwnPropDesc(from, key)) || desc.enumerable });
  }
  return to;
};
var __toESM = (mod, isNodeMode, target) => (target = mod != null ? __create(__getProtoOf(mod)) : {}, __copyProps(
  // If the importer is in node compatibility mode or this is not an ESM
  // file that has been converted to a CommonJS file using a Babel-
  // compatible transform (i.e. "__esModule" has not been set), then set
  // "default" to the CommonJS "module.exports" for node compatibility.
  isNodeMode || !mod || !mod.__esModule ? __defProp(target, "default", { value: mod, enumerable: true }) : target,
  mod
));
var __toCommonJS = (mod) => __copyProps(__defProp({}, "__esModule", { value: true }), mod);

// ../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/@forge/api/out/api/errors.js
var require_errors = __commonJS({
  "../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/@forge/api/out/api/errors.js"(exports2) {
    "use strict";
    Object.defineProperty(exports2, "__esModule", { value: true });
    exports2.ApiNotReadyError = exports2.ProxyRequestError = exports2.InvalidContainerServiceError = exports2.InvalidRemoteError = exports2.NeedsAuthenticationError = exports2.InvalidWorkspaceRequestedError = exports2.RequestProductNotAllowedError = exports2.ProductEndpointNotAllowedError = exports2.ExternalEndpointNotAllowedError = exports2.NotAllowedError = exports2.FetchError = exports2.HttpError = exports2.API_NOT_READY_ERR = exports2.PROXY_ERR = exports2.INVALID_CONTAINER_SERVICE_ERR = exports2.INVALID_REMOTE_ERR = exports2.NEEDS_AUTHENTICATION_ERR = exports2.FUNCTION_FETCH_ERR = exports2.REQUEST_EGRESS_ALLOWLIST_ERR = exports2.FUNCTION_ERR = void 0;
    exports2.isForgePlatformError = isForgePlatformError;
    exports2.isHostedCodeError = isHostedCodeError;
    exports2.isExpectedError = isExpectedError;
    exports2.FUNCTION_ERR = "FUNCTION_ERR";
    exports2.REQUEST_EGRESS_ALLOWLIST_ERR = "REQUEST_EGRESS_ALLOWLIST_ERR";
    exports2.FUNCTION_FETCH_ERR = "FUNCTION_FETCH_ERR";
    exports2.NEEDS_AUTHENTICATION_ERR = "NEEDS_AUTHENTICATION_ERR";
    exports2.INVALID_REMOTE_ERR = "INVALID_REMOTE_ERR";
    exports2.INVALID_CONTAINER_SERVICE_ERR = "INVALID_CONTAINER_SERVICE_ERR";
    exports2.PROXY_ERR = "PROXY_ERR";
    exports2.API_NOT_READY_ERR = "API_NOT_READY_ERR";
    function isForgePlatformError(err) {
      return [
        exports2.REQUEST_EGRESS_ALLOWLIST_ERR,
        exports2.FUNCTION_FETCH_ERR,
        exports2.NEEDS_AUTHENTICATION_ERR,
        exports2.PROXY_ERR,
        exports2.API_NOT_READY_ERR
      ].includes(err.name);
    }
    function isHostedCodeError(err) {
      return [exports2.FUNCTION_ERR, exports2.REQUEST_EGRESS_ALLOWLIST_ERR, exports2.FUNCTION_FETCH_ERR, exports2.NEEDS_AUTHENTICATION_ERR].includes(typeof err === "string" ? err : err.name);
    }
    function isExpectedError(err) {
      return err.name === exports2.NEEDS_AUTHENTICATION_ERR && !!err.options?.isExpectedError;
    }
    var HttpError = class extends Error {
      status;
      constructor(message) {
        super(message);
      }
    };
    exports2.HttpError = HttpError;
    var FetchError = class extends Error {
      constructor(cause) {
        super(cause);
        this.stack = void 0;
        this.name = exports2.FUNCTION_FETCH_ERR;
      }
    };
    exports2.FetchError = FetchError;
    var NotAllowedError = class extends HttpError {
      constructor(message) {
        super(message);
        this.stack = void 0;
        this.name = exports2.REQUEST_EGRESS_ALLOWLIST_ERR;
        this.status = 403;
      }
    };
    exports2.NotAllowedError = NotAllowedError;
    var ExternalEndpointNotAllowedError = class extends NotAllowedError {
      constructor(failedURL) {
        super(`URL not included in the external fetch backend permissions: ${failedURL}. Visit go.atlassian.com/forge-egress for more information.`);
      }
    };
    exports2.ExternalEndpointNotAllowedError = ExternalEndpointNotAllowedError;
    var ProductEndpointNotAllowedError = class extends NotAllowedError {
      constructor(failedURL) {
        super(`URL not allowed: ${failedURL}.`);
      }
    };
    exports2.ProductEndpointNotAllowedError = ProductEndpointNotAllowedError;
    var RequestProductNotAllowedError = class extends NotAllowedError {
      constructor(requestedProduct, invocationProduct) {
        super(`Request ${requestedProduct} is not allowed from ${invocationProduct} context.`);
      }
    };
    exports2.RequestProductNotAllowedError = RequestProductNotAllowedError;
    var InvalidWorkspaceRequestedError = class extends NotAllowedError {
      constructor(failedURL) {
        super(`Invalid workspace requested in URL: ${failedURL}.`);
      }
    };
    exports2.InvalidWorkspaceRequestedError = InvalidWorkspaceRequestedError;
    var NeedsAuthenticationError = class extends HttpError {
      serviceKey;
      options;
      constructor(error, serviceKey, options) {
        super(error);
        this.serviceKey = serviceKey;
        this.options = options;
        this.stack = void 0;
        this.name = exports2.NEEDS_AUTHENTICATION_ERR;
        this.status = 401;
      }
    };
    exports2.NeedsAuthenticationError = NeedsAuthenticationError;
    var InvalidRemoteError = class extends HttpError {
      remoteKey;
      constructor(error, remoteKey) {
        super(error);
        this.remoteKey = remoteKey;
        this.name = exports2.INVALID_REMOTE_ERR;
        this.status = 400;
      }
    };
    exports2.InvalidRemoteError = InvalidRemoteError;
    var InvalidContainerServiceError = class extends HttpError {
      serviceKey;
      constructor(error, serviceKey) {
        super(error);
        this.serviceKey = serviceKey;
        this.name = exports2.INVALID_CONTAINER_SERVICE_ERR;
        this.status = 400;
      }
    };
    exports2.InvalidContainerServiceError = InvalidContainerServiceError;
    var ProxyRequestError = class extends HttpError {
      status;
      errorCode;
      constructor(status, errorCode) {
        super(`Forge platform failed to process runtime HTTP request - ${status} - ${errorCode}`);
        this.status = status;
        this.errorCode = errorCode;
        this.name = exports2.PROXY_ERR;
      }
    };
    exports2.ProxyRequestError = ProxyRequestError;
    var ApiNotReadyError = class extends Error {
      constructor(message = "Forge API currently not available") {
        super(message);
        this.name = exports2.API_NOT_READY_ERR;
      }
    };
    exports2.ApiNotReadyError = ApiNotReadyError;
  }
});

// ../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/@forge/api/out/safeUrl.js
var require_safeUrl = __commonJS({
  "../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/@forge/api/out/safeUrl.js"(exports2) {
    "use strict";
    Object.defineProperty(exports2, "__esModule", { value: true });
    exports2.isRoute = isRoute;
    exports2.routeFromAbsolute = routeFromAbsolute;
    exports2.route = route2;
    exports2.requireSafeUrl = requireSafeUrl;
    exports2.assumeTrustedRoute = assumeTrustedRoute;
    var ReadonlyRoute = class {
      value_;
      constructor(value_) {
        this.value_ = value_;
      }
      set value(_) {
        throw new Error("modification of a Route is not allowed");
      }
      get value() {
        return this.value_;
      }
    };
    function isRoute(x) {
      return x instanceof ReadonlyRoute;
    }
    function routeFromAbsolute(absolutePath) {
      const absoluteURL = new URL(absolutePath);
      return assumeTrustedRoute(`${absoluteURL.pathname}${absoluteURL.search}`);
    }
    var DOUBLE_DOT = ["..", ".%2e", "%2e.", "%2e%2e", ".%2E", "%2E.", "%2E%2e"];
    var DIRECTORY_PATH = ["/", "\\"];
    var ENDS_PATH = ["?", "#"];
    function containsOneOf(needles, haystack) {
      return needles.some((needle) => haystack.includes(needle));
    }
    function escapeParameter(parameter, mode) {
      switch (mode) {
        case "path":
          if (isRoute(parameter)) {
            return parameter.value;
          }
          parameter = String(parameter);
          if (containsOneOf(DOUBLE_DOT, parameter) || containsOneOf(ENDS_PATH, parameter) || containsOneOf(DIRECTORY_PATH, parameter)) {
            throw new Error("Disallowing path manipulation attempt. For more information see: https://go.atlassian.com/product-fetch-api-route");
          }
          return parameter;
        case "query":
          if (isRoute(parameter)) {
            return encodeURIComponent(parameter.value);
          } else if (parameter instanceof URLSearchParams) {
            return parameter.toString();
          } else {
            return encodeURIComponent(parameter);
          }
      }
    }
    function route2(template, ...parameters) {
      let mode = "path";
      let result = "";
      for (let i = 0; i < template.length; i++) {
        const templateFragment = template[i];
        if (containsOneOf(ENDS_PATH, templateFragment)) {
          mode = "query";
        }
        result += templateFragment;
        if (i >= parameters.length) {
          break;
        }
        result += escapeParameter(parameters[i], mode);
      }
      return new ReadonlyRoute(result);
    }
    function requireSafeUrl(url) {
      if (url instanceof ReadonlyRoute) {
        return url;
      }
      throw new Error(`You must create your route using the 'route' export from '@forge/api'.
See https://go.atlassian.com/forge-fetch-route for more information.`);
    }
    function assumeTrustedRoute(route3) {
      return new ReadonlyRoute(route3);
    }
  }
});

// ../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/@forge/api/out/api/ari.js
var require_ari = __commonJS({
  "../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/@forge/api/out/api/ari.js"(exports2) {
    "use strict";
    Object.defineProperty(exports2, "__esModule", { value: true });
    exports2.getInstallationAri = exports2.getEnvironmentAri = exports2.getAppAri = void 0;
    var getAppAri = (appId) => ({
      appId,
      toString: () => `ari:cloud:ecosystem::app/${appId}`,
      toJSON: () => `ari:cloud:ecosystem::app/${appId}`
    });
    exports2.getAppAri = getAppAri;
    var getEnvironmentAri = (appId, environmentId) => ({
      environmentId,
      toString: () => `ari:cloud:ecosystem::environment/${appId}/${environmentId}`,
      toJSON: () => `ari:cloud:ecosystem::environment/${appId}/${environmentId}`
    });
    exports2.getEnvironmentAri = getEnvironmentAri;
    var getInstallationAri = (installationId) => ({
      installationId,
      toString: () => `ari:cloud:ecosystem::installation/${installationId}`,
      toJSON: () => `ari:cloud:ecosystem::installation/${installationId}`
    });
    exports2.getInstallationAri = getInstallationAri;
  }
});

// ../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/@forge/api/out/api/runtime.js
var require_runtime = __commonJS({
  "../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/@forge/api/out/api/runtime.js"(exports2) {
    "use strict";
    Object.defineProperty(exports2, "__esModule", { value: true });
    exports2.MissingPermissions = exports2.PermissionRequirements = exports2.RuntimePermissions = exports2.Permissions = void 0;
    exports2.__getRuntime = __getRuntime;
    exports2.getAppContext = getAppContext;
    exports2.wrapInMetrics = wrapInMetrics;
    exports2.bindInvocationContext = bindInvocationContext;
    var errors_1 = require_errors();
    var ari_1 = require_ari();
    var extractUrlString = (item) => {
      if (typeof item === "string") {
        return item;
      } else if ("address" in item) {
        return item.address;
      } else {
        return item.remote;
      }
    };
    var formatScopesSection = (scopes) => {
      if (scopes && Array.isArray(scopes) && scopes.length > 0) {
        return `Scopes: ${scopes.join(", ")}`;
      }
      return null;
    };
    var formatExternalSection = (external) => {
      if (!external) {
        return null;
      }
      const externalParts = [];
      Object.keys(external).forEach((type) => {
        if (type === "fetch") {
          const fetchParts = getFetchPermissions(external.fetch);
          externalParts.push(...fetchParts);
        } else {
          const externalUrls = external[type];
          if (externalUrls && Array.isArray(externalUrls) && externalUrls.length > 0) {
            const capitalizedType = String(type).charAt(0).toUpperCase() + String(type).slice(1);
            const urlList = externalUrls.map(extractUrlString).join(", ");
            externalParts.push(`${capitalizedType}: ${urlList}`);
          }
        }
      });
      return externalParts.length > 0 ? `External: ${externalParts.join("; ")}` : null;
    };
    var getFetchPermissions = (fetch2) => {
      if (!fetch2) {
        return [];
      }
      const fetchParts = [];
      Object.keys(fetch2).forEach((fetchType) => {
        const urls = fetch2[fetchType];
        if (urls && urls.length > 0) {
          const urlList = urls.map(extractUrlString).join(", ");
          const capitalizedType = String(fetchType).charAt(0).toUpperCase() + String(fetchType).slice(1);
          fetchParts.push(`Fetch ${capitalizedType}: ${urlList}`);
        }
      });
      return fetchParts;
    };
    var Permissions = class {
      format() {
        const parts = [];
        const scopesSection = formatScopesSection(this.scopes);
        if (scopesSection) {
          parts.push(scopesSection);
        }
        const externalSection = formatExternalSection(this.external);
        if (externalSection) {
          parts.push(externalSection);
        }
        return parts.length > 0 ? parts.join("; ") : "No permissions specified";
      }
    };
    exports2.Permissions = Permissions;
    var RuntimePermissions = class extends Permissions {
      scopes;
      external;
      constructor(scopes, external) {
        super();
        this.scopes = scopes;
        this.external = external;
      }
    };
    exports2.RuntimePermissions = RuntimePermissions;
    var PermissionRequirements = class extends Permissions {
      scopes;
      external;
      constructor(scopes, external) {
        super();
        this.scopes = scopes;
        this.external = external;
      }
    };
    exports2.PermissionRequirements = PermissionRequirements;
    var MissingPermissions = class extends Permissions {
      scopes;
      external;
      constructor(scopes, external) {
        super();
        this.scopes = scopes;
        this.external = external;
      }
    };
    exports2.MissingPermissions = MissingPermissions;
    function __getRuntime() {
      const runtime = global.__forge_runtime__;
      if (!runtime) {
        throw new Error("Forge runtime not found.");
      }
      return runtime;
    }
    function getAppContext() {
      const runtime = __getRuntime();
      const { appId, appVersion, environmentId, environmentType, invocationId, installationId, moduleKey, license, installation, permissions } = runtime.appContext;
      const invocationRemainingTimeInMillis = runtime.lambdaContext.getRemainingTimeInMillis ?? (() => {
        throw new Error("Lambda remaining time is not available. If tunnelling, update Forge CLI to the latest version.");
      });
      return {
        appAri: (0, ari_1.getAppAri)(appId),
        appVersion,
        environmentAri: (0, ari_1.getEnvironmentAri)(appId, environmentId),
        environmentType,
        installationAri: (0, ari_1.getInstallationAri)(installationId),
        invocationId,
        invocationRemainingTimeInMillis,
        moduleKey,
        license,
        installation,
        permissions: new RuntimePermissions(permissions?.scopes || [], permissions?.external || {})
      };
    }
    function wrapInMetrics(name, fn) {
      return async (...args) => {
        const { metrics } = __getRuntime();
        metrics.counter(name).incr();
        const timer = metrics.timing(name).measure();
        let success = true;
        try {
          return await fn(...args);
        } catch (e) {
          const undiciError = global.__forge_undici_error__;
          if (e instanceof errors_1.ProxyRequestError || undiciError && typeof undiciError === "function" && e instanceof undiciError) {
            success = false;
          }
          throw e;
        } finally {
          timer.stop({ success: success.toString() });
        }
      };
    }
    function bindInvocationContext(fn) {
      const AsyncLocalStorage = require("async_hooks").AsyncLocalStorage;
      return AsyncLocalStorage.bind(fn);
    }
  }
});

// ../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/@forge/api/out/api/fetch.js
var require_fetch = __commonJS({
  "../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/@forge/api/out/api/fetch.js"(exports2) {
    "use strict";
    Object.defineProperty(exports2, "__esModule", { value: true });
    exports2.__requestAtlassianAsUser = exports2.__requestAtlassianAsApp = exports2.wrapRequestConnectedData = exports2.wrapRequestTeamworkGraph = exports2.wrapRequestGraph = exports2.handleProxyResponseErrors = exports2.getForgeProxyError = void 0;
    exports2.__fetchProduct = __fetchProduct;
    exports2.fetchRemote = fetchRemote;
    exports2.getFetchAPI = getFetchAPI;
    var safeUrl_1 = require_safeUrl();
    var errors_1 = require_errors();
    var runtime_1 = require_runtime();
    async function wrapInMetrics(options, cb) {
      const metrics = (0, runtime_1.__getRuntime)().metrics;
      metrics.counter(options.name, options.tags).incr();
      const timer = metrics.timing(options.name, options.tags).measure();
      try {
        return await cb();
      } finally {
        timer.stop();
      }
    }
    function __fetchProduct(args) {
      return async (path, init) => {
        const response = await global.__forge_fetch__({
          type: args.type,
          provider: args.provider,
          accountId: args.accountId,
          ...args.remote ? { remote: args.remote } : {},
          ...args.contextAri ? { contextAri: args.contextAri } : {}
        }, path, init);
        (0, exports2.handleProxyResponseErrors)(response);
        return response;
      };
    }
    function fetchRemote(args) {
      return async (path, init) => {
        const response = await global.__forge_fetch__({
          type: "tpp",
          provider: args.provider,
          remote: args.remote,
          accountId: args.account
        }, path, init);
        (0, exports2.handleProxyResponseErrors)(response);
        return response;
      };
    }
    function getDefaultRemote(provider) {
      const externalAuthProvider = findExternalAuthProviderConfigOrThrow(provider);
      if (!externalAuthProvider.remotes.length) {
        throw new Error(`Missing remote config for provider ${provider}`);
      }
      return externalAuthProvider.remotes[0].key;
    }
    function findExternalAuthProviderConfigOrThrow(provider) {
      const { externalAuth } = (0, runtime_1.__getRuntime)();
      const externalAuthProvider = externalAuth?.find((externalAuthMetaData) => {
        return externalAuthMetaData.service === provider;
      });
      if (!externalAuthProvider) {
        throw new Error(`Bad provider or missing config for provider ${provider}`);
      }
      return externalAuthProvider;
    }
    var ATLASSIAN_TOKEN_SERVICE_KEY = "atlassian-token-service-key";
    var getForgeProxyError = (response) => response.headers.get("forge-proxy-error");
    exports2.getForgeProxyError = getForgeProxyError;
    var handleProxyResponseErrors = (response) => {
      const errorReason = (0, exports2.getForgeProxyError)(response);
      if (errorReason) {
        if (errorReason === "NEEDS_AUTHENTICATION_ERR") {
          throw new errors_1.NeedsAuthenticationError("Authentication Required", ATLASSIAN_TOKEN_SERVICE_KEY);
        }
        throw new errors_1.ProxyRequestError(response.status, errorReason);
      }
    };
    exports2.handleProxyResponseErrors = handleProxyResponseErrors;
    function lazyThrowNeedsAuthenticationError(serviceKey) {
      return async (scopes) => wrapInMetrics({ name: "api.asUser.withProvider.requestCredentials", tags: { passingScopes: String(!!scopes) } }, async () => {
        throw new errors_1.NeedsAuthenticationError("Authentication Required", serviceKey, { scopes, isExpectedError: true });
      });
    }
    function buildExternalAuthAccountsInfo(provider, remote) {
      const { accounts } = findExternalAuthProviderConfigOrThrow(provider);
      const buildAccountModel = (account) => {
        const { externalAccountId: id, ...rest } = account;
        return { ...rest, id };
      };
      const buildExternalAuthAccountMethods = (account, outboundAuthAccountId) => ({
        hasCredentials: async (scopes) => wrapInMetrics({ name: "api.asUser.withProvider.hasCredentials", tags: { passingScopes: String(!!scopes) } }, async () => !scopes || scopes.every((scope) => account.scopes.includes(scope))),
        requestCredentials: lazyThrowNeedsAuthenticationError(provider),
        getAccount: async () => wrapInMetrics({ name: "api.asUser.withProvider.getAccount" }, async () => account),
        fetch: wrapWithRouteUnwrapper(fetchRemote({ provider, remote: remote ?? getDefaultRemote(provider), account: outboundAuthAccountId }))
      });
      return accounts.map((account) => {
        const authAccount = buildAccountModel(account);
        return {
          account: authAccount,
          methods: buildExternalAuthAccountMethods(authAccount, account.id)
        };
      });
    }
    var throwNotImplementedError = () => {
      throw new Error("not implemented");
    };
    var withProvider = (provider, remote) => {
      const accountsInfo = buildExternalAuthAccountsInfo(provider, remote);
      const defaultAccountInfo = accountsInfo.length ? accountsInfo[0] : void 0;
      const lazyThrowNoValidCredentialsError = () => {
        return (url) => {
          throw new Error(`Fetch failed for ${remote ? `remote '${remote}', ` : ""}provider '${provider}', path '${url}' no credentials previously requested`);
        };
      };
      return {
        hasCredentials: async (scopes) => {
          return defaultAccountInfo ? await defaultAccountInfo.methods.hasCredentials(scopes) : await wrapInMetrics({ name: "api.asUser.withProvider.hasCredentials", tags: { passingScopes: String(!!scopes) } }, async () => false);
        },
        getAccount: async () => wrapInMetrics({ name: "api.asUser.withProvider.getAccount" }, async () => {
          return defaultAccountInfo ? defaultAccountInfo.account : void 0;
        }),
        requestCredentials: lazyThrowNeedsAuthenticationError(provider),
        listCredentials: throwNotImplementedError,
        listAccounts: async () => wrapInMetrics({ name: "api.asUser.withProvider.listAccounts" }, async () => {
          return accountsInfo.map(({ account }) => account);
        }),
        asAccount: (externalAccountId) => {
          const accountInfo = accountsInfo.find(({ account }) => account.id === externalAccountId);
          if (!accountInfo) {
            throw new Error(`No account with ID ${externalAccountId} found for provider ${provider}`);
          }
          return accountInfo.methods;
        },
        fetch: defaultAccountInfo ? defaultAccountInfo.methods.fetch : lazyThrowNoValidCredentialsError()
      };
    };
    var withContext = (provider, accountId) => (contextAri) => {
      const requestProductFn = wrapRequestProduct(__fetchProduct({ provider, contextAri, type: "fpp", accountId }));
      return {
        requestJira: requestProductFn,
        requestConfluence: requestProductFn,
        requestBitbucket: requestProductFn
      };
    };
    var wrapWithRouteUnwrapper = (fetch2) => (path, init) => {
      const stringPath = (0, safeUrl_1.isRoute)(path) ? path.value : path;
      return fetch2(stringPath, init);
    };
    var wrapRequestProduct = (requestProduct) => (path, init) => {
      const safeUrl = (0, safeUrl_1.requireSafeUrl)(path);
      return requestProduct(safeUrl.value, init);
    };
    var wrapRequestGraph = (requestGraphApi) => (query, variables, headers = {}) => requestGraphApi("/graphql", {
      method: "POST",
      headers: { ...headers, "Content-Type": "application/json" },
      body: JSON.stringify({
        query,
        ...variables ? { variables } : {}
      })
    });
    exports2.wrapRequestGraph = wrapRequestGraph;
    var wrapRequestTeamworkGraph = (requestGraphApi) => (query, variables, operationName, extensions, headers = {}) => requestGraphApi("/graphql/twg", {
      method: "POST",
      headers: { ...headers, "Content-Type": "application/json" },
      body: JSON.stringify({
        query,
        ...variables ? { variables } : {},
        ...operationName ? { operationName } : {},
        ...extensions ? { extensions } : {}
      })
    });
    exports2.wrapRequestTeamworkGraph = wrapRequestTeamworkGraph;
    var wrapRequestConnectedData = (fetch2) => (path, init) => {
      const safeUrl = (0, safeUrl_1.requireSafeUrl)(path);
      return fetch2(`/connected-data/${safeUrl.value.replace(/^\/+/, "")}`, init);
    };
    exports2.wrapRequestConnectedData = wrapRequestConnectedData;
    function getFetchAPI() {
      if (global.fetch === void 0) {
        global.fetch = async () => {
          throw new Error("The fetch function is not available");
        };
      }
      return {
        fetch: wrapWithRouteUnwrapper(fetch),
        requestJira: wrapRequestProduct(__fetchProduct({ provider: "none", remote: "jira", type: "fpp" })),
        requestConfluence: wrapRequestProduct(__fetchProduct({ provider: "none", remote: "confluence", type: "fpp" })),
        requestBitbucket: wrapRequestProduct(__fetchProduct({ provider: "none", remote: "bitbucket", type: "fpp" })),
        asUser: (userId) => ({
          requestJira: wrapRequestProduct(__fetchProduct({ provider: "user", remote: "jira", type: "fpp", accountId: userId })),
          requestConfluence: wrapRequestProduct(__fetchProduct({ provider: "user", remote: "confluence", type: "fpp", accountId: userId })),
          requestBitbucket: wrapRequestProduct(__fetchProduct({ provider: "user", remote: "bitbucket", type: "fpp", accountId: userId })),
          requestGraph: (0, exports2.wrapRequestGraph)(__fetchProduct({ provider: "user", remote: "stargate", type: "fpp", accountId: userId })),
          requestTeamworkGraph: (0, exports2.wrapRequestTeamworkGraph)(__fetchProduct({ provider: "user", remote: "stargate", type: "fpp", accountId: userId })),
          requestConnectedData: (0, exports2.wrapRequestConnectedData)(__fetchProduct({ provider: "user", remote: "stargate", type: "fpp" })),
          requestAtlassian: wrapRequestProduct(__fetchProduct({ provider: "user", remote: "stargate", type: "fpp", accountId: userId })),
          withProvider,
          withContext: withContext("user", userId)
        }),
        asApp: () => ({
          requestJira: wrapRequestProduct(__fetchProduct({ provider: "app", remote: "jira", type: "fpp" })),
          requestConfluence: wrapRequestProduct(__fetchProduct({ provider: "app", remote: "confluence", type: "fpp" })),
          requestBitbucket: wrapRequestProduct(__fetchProduct({ provider: "app", remote: "bitbucket", type: "fpp" })),
          requestGraph: (0, exports2.wrapRequestGraph)(__fetchProduct({ provider: "app", remote: "stargate", type: "fpp" })),
          requestConnectedData: (0, exports2.wrapRequestConnectedData)(__fetchProduct({ provider: "app", remote: "stargate", type: "fpp" })),
          requestAtlassian: wrapRequestProduct(__fetchProduct({ provider: "app", remote: "stargate", type: "fpp" })),
          withContext: withContext("app")
        })
      };
    }
    function getRequestStargate(provider) {
      if (provider !== "app" && provider !== "user") {
        throw new Error(`Unsupported provider: ${provider}`);
      }
      return __fetchProduct({ provider, remote: "stargate", type: "fpp" });
    }
    exports2.__requestAtlassianAsApp = getRequestStargate("app");
    exports2.__requestAtlassianAsUser = getRequestStargate("user");
  }
});

// ../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/@forge/api/out/api/endpoint.js
var require_endpoint = __commonJS({
  "../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/@forge/api/out/api/endpoint.js"(exports2) {
    "use strict";
    Object.defineProperty(exports2, "__esModule", { value: true });
    exports2.invokeRemote = invokeRemote;
    exports2.invokeService = invokeService;
    var errors_1 = require_errors();
    var fetch_1 = require_fetch();
    var InvokeType;
    (function(InvokeType2) {
      InvokeType2["REMOTE"] = "Remote";
      InvokeType2["CONTAINER"] = "Service";
    })(InvokeType || (InvokeType = {}));
    async function invokeRemote(remoteKey, options) {
      return invokeEndpoint(remoteKey, options, InvokeType.REMOTE);
    }
    async function invokeService(serviceKey, options) {
      return invokeEndpoint(serviceKey, options, InvokeType.CONTAINER);
    }
    async function invokeEndpoint(key, options, type) {
      const { path, ...fetchOptions } = options;
      if (!key) {
        throw new Error(`Missing ${type.toLowerCase()} key provided to invoke${type}`);
      }
      if (!path) {
        throw new Error(`Missing or empty path provided to invoke${type}`);
      }
      const response = await global.__forge_fetch__(constructInvokePayload(key, type), path, fetchOptions);
      handleResponseErrors(response, key);
      return response;
    }
    function constructInvokePayload(key, type) {
      switch (type) {
        case InvokeType.REMOTE:
          return {
            type: "frc",
            remote: key
          };
        case InvokeType.CONTAINER:
          return {
            type: "fcc",
            service: key
          };
      }
    }
    function handleResponseErrors(response, key) {
      const forgeProxyError = (0, fetch_1.getForgeProxyError)(response);
      if (forgeProxyError === "INVALID_SERVICE_KEY") {
        throw new errors_1.InvalidContainerServiceError(`Invalid service key provided: "${key}"`, key);
      } else if (forgeProxyError === "INVALID_REMOTE") {
        throw new errors_1.InvalidRemoteError(`Invalid remote key provided: "${key}"`, key);
      }
      (0, fetch_1.handleProxyResponseErrors)(response);
    }
  }
});

// ../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/tslib/tslib.es6.mjs
var tslib_es6_exports = {};
__export(tslib_es6_exports, {
  __addDisposableResource: () => __addDisposableResource,
  __assign: () => __assign,
  __asyncDelegator: () => __asyncDelegator,
  __asyncGenerator: () => __asyncGenerator,
  __asyncValues: () => __asyncValues,
  __await: () => __await,
  __awaiter: () => __awaiter,
  __classPrivateFieldGet: () => __classPrivateFieldGet,
  __classPrivateFieldIn: () => __classPrivateFieldIn,
  __classPrivateFieldSet: () => __classPrivateFieldSet,
  __createBinding: () => __createBinding,
  __decorate: () => __decorate,
  __disposeResources: () => __disposeResources,
  __esDecorate: () => __esDecorate,
  __exportStar: () => __exportStar,
  __extends: () => __extends,
  __generator: () => __generator,
  __importDefault: () => __importDefault,
  __importStar: () => __importStar,
  __makeTemplateObject: () => __makeTemplateObject,
  __metadata: () => __metadata,
  __param: () => __param,
  __propKey: () => __propKey,
  __read: () => __read,
  __rest: () => __rest,
  __rewriteRelativeImportExtension: () => __rewriteRelativeImportExtension,
  __runInitializers: () => __runInitializers,
  __setFunctionName: () => __setFunctionName,
  __spread: () => __spread,
  __spreadArray: () => __spreadArray,
  __spreadArrays: () => __spreadArrays,
  __values: () => __values,
  default: () => tslib_es6_default
});
function __extends(d, b) {
  if (typeof b !== "function" && b !== null)
    throw new TypeError("Class extends value " + String(b) + " is not a constructor or null");
  extendStatics(d, b);
  function __() {
    this.constructor = d;
  }
  d.prototype = b === null ? Object.create(b) : (__.prototype = b.prototype, new __());
}
function __rest(s, e) {
  var t = {};
  for (var p in s) if (Object.prototype.hasOwnProperty.call(s, p) && e.indexOf(p) < 0)
    t[p] = s[p];
  if (s != null && typeof Object.getOwnPropertySymbols === "function")
    for (var i = 0, p = Object.getOwnPropertySymbols(s); i < p.length; i++) {
      if (e.indexOf(p[i]) < 0 && Object.prototype.propertyIsEnumerable.call(s, p[i]))
        t[p[i]] = s[p[i]];
    }
  return t;
}
function __decorate(decorators, target, key, desc) {
  var c = arguments.length, r = c < 3 ? target : desc === null ? desc = Object.getOwnPropertyDescriptor(target, key) : desc, d;
  if (typeof Reflect === "object" && typeof Reflect.decorate === "function") r = Reflect.decorate(decorators, target, key, desc);
  else for (var i = decorators.length - 1; i >= 0; i--) if (d = decorators[i]) r = (c < 3 ? d(r) : c > 3 ? d(target, key, r) : d(target, key)) || r;
  return c > 3 && r && Object.defineProperty(target, key, r), r;
}
function __param(paramIndex, decorator) {
  return function(target, key) {
    decorator(target, key, paramIndex);
  };
}
function __esDecorate(ctor, descriptorIn, decorators, contextIn, initializers, extraInitializers) {
  function accept(f) {
    if (f !== void 0 && typeof f !== "function") throw new TypeError("Function expected");
    return f;
  }
  var kind = contextIn.kind, key = kind === "getter" ? "get" : kind === "setter" ? "set" : "value";
  var target = !descriptorIn && ctor ? contextIn["static"] ? ctor : ctor.prototype : null;
  var descriptor = descriptorIn || (target ? Object.getOwnPropertyDescriptor(target, contextIn.name) : {});
  var _, done = false;
  for (var i = decorators.length - 1; i >= 0; i--) {
    var context = {};
    for (var p in contextIn) context[p] = p === "access" ? {} : contextIn[p];
    for (var p in contextIn.access) context.access[p] = contextIn.access[p];
    context.addInitializer = function(f) {
      if (done) throw new TypeError("Cannot add initializers after decoration has completed");
      extraInitializers.push(accept(f || null));
    };
    var result = (0, decorators[i])(kind === "accessor" ? { get: descriptor.get, set: descriptor.set } : descriptor[key], context);
    if (kind === "accessor") {
      if (result === void 0) continue;
      if (result === null || typeof result !== "object") throw new TypeError("Object expected");
      if (_ = accept(result.get)) descriptor.get = _;
      if (_ = accept(result.set)) descriptor.set = _;
      if (_ = accept(result.init)) initializers.unshift(_);
    } else if (_ = accept(result)) {
      if (kind === "field") initializers.unshift(_);
      else descriptor[key] = _;
    }
  }
  if (target) Object.defineProperty(target, contextIn.name, descriptor);
  done = true;
}
function __runInitializers(thisArg, initializers, value) {
  var useValue = arguments.length > 2;
  for (var i = 0; i < initializers.length; i++) {
    value = useValue ? initializers[i].call(thisArg, value) : initializers[i].call(thisArg);
  }
  return useValue ? value : void 0;
}
function __propKey(x) {
  return typeof x === "symbol" ? x : "".concat(x);
}
function __setFunctionName(f, name, prefix) {
  if (typeof name === "symbol") name = name.description ? "[".concat(name.description, "]") : "";
  return Object.defineProperty(f, "name", { configurable: true, value: prefix ? "".concat(prefix, " ", name) : name });
}
function __metadata(metadataKey, metadataValue) {
  if (typeof Reflect === "object" && typeof Reflect.metadata === "function") return Reflect.metadata(metadataKey, metadataValue);
}
function __awaiter(thisArg, _arguments, P, generator) {
  function adopt(value) {
    return value instanceof P ? value : new P(function(resolve) {
      resolve(value);
    });
  }
  return new (P || (P = Promise))(function(resolve, reject) {
    function fulfilled(value) {
      try {
        step(generator.next(value));
      } catch (e) {
        reject(e);
      }
    }
    function rejected(value) {
      try {
        step(generator["throw"](value));
      } catch (e) {
        reject(e);
      }
    }
    function step(result) {
      result.done ? resolve(result.value) : adopt(result.value).then(fulfilled, rejected);
    }
    step((generator = generator.apply(thisArg, _arguments || [])).next());
  });
}
function __generator(thisArg, body) {
  var _ = { label: 0, sent: function() {
    if (t[0] & 1) throw t[1];
    return t[1];
  }, trys: [], ops: [] }, f, y, t, g = Object.create((typeof Iterator === "function" ? Iterator : Object).prototype);
  return g.next = verb(0), g["throw"] = verb(1), g["return"] = verb(2), typeof Symbol === "function" && (g[Symbol.iterator] = function() {
    return this;
  }), g;
  function verb(n) {
    return function(v) {
      return step([n, v]);
    };
  }
  function step(op) {
    if (f) throw new TypeError("Generator is already executing.");
    while (g && (g = 0, op[0] && (_ = 0)), _) try {
      if (f = 1, y && (t = op[0] & 2 ? y["return"] : op[0] ? y["throw"] || ((t = y["return"]) && t.call(y), 0) : y.next) && !(t = t.call(y, op[1])).done) return t;
      if (y = 0, t) op = [op[0] & 2, t.value];
      switch (op[0]) {
        case 0:
        case 1:
          t = op;
          break;
        case 4:
          _.label++;
          return { value: op[1], done: false };
        case 5:
          _.label++;
          y = op[1];
          op = [0];
          continue;
        case 7:
          op = _.ops.pop();
          _.trys.pop();
          continue;
        default:
          if (!(t = _.trys, t = t.length > 0 && t[t.length - 1]) && (op[0] === 6 || op[0] === 2)) {
            _ = 0;
            continue;
          }
          if (op[0] === 3 && (!t || op[1] > t[0] && op[1] < t[3])) {
            _.label = op[1];
            break;
          }
          if (op[0] === 6 && _.label < t[1]) {
            _.label = t[1];
            t = op;
            break;
          }
          if (t && _.label < t[2]) {
            _.label = t[2];
            _.ops.push(op);
            break;
          }
          if (t[2]) _.ops.pop();
          _.trys.pop();
          continue;
      }
      op = body.call(thisArg, _);
    } catch (e) {
      op = [6, e];
      y = 0;
    } finally {
      f = t = 0;
    }
    if (op[0] & 5) throw op[1];
    return { value: op[0] ? op[1] : void 0, done: true };
  }
}
function __exportStar(m, o) {
  for (var p in m) if (p !== "default" && !Object.prototype.hasOwnProperty.call(o, p)) __createBinding(o, m, p);
}
function __values(o) {
  var s = typeof Symbol === "function" && Symbol.iterator, m = s && o[s], i = 0;
  if (m) return m.call(o);
  if (o && typeof o.length === "number") return {
    next: function() {
      if (o && i >= o.length) o = void 0;
      return { value: o && o[i++], done: !o };
    }
  };
  throw new TypeError(s ? "Object is not iterable." : "Symbol.iterator is not defined.");
}
function __read(o, n) {
  var m = typeof Symbol === "function" && o[Symbol.iterator];
  if (!m) return o;
  var i = m.call(o), r, ar = [], e;
  try {
    while ((n === void 0 || n-- > 0) && !(r = i.next()).done) ar.push(r.value);
  } catch (error) {
    e = { error };
  } finally {
    try {
      if (r && !r.done && (m = i["return"])) m.call(i);
    } finally {
      if (e) throw e.error;
    }
  }
  return ar;
}
function __spread() {
  for (var ar = [], i = 0; i < arguments.length; i++)
    ar = ar.concat(__read(arguments[i]));
  return ar;
}
function __spreadArrays() {
  for (var s = 0, i = 0, il = arguments.length; i < il; i++) s += arguments[i].length;
  for (var r = Array(s), k = 0, i = 0; i < il; i++)
    for (var a = arguments[i], j = 0, jl = a.length; j < jl; j++, k++)
      r[k] = a[j];
  return r;
}
function __spreadArray(to, from, pack) {
  if (pack || arguments.length === 2) for (var i = 0, l = from.length, ar; i < l; i++) {
    if (ar || !(i in from)) {
      if (!ar) ar = Array.prototype.slice.call(from, 0, i);
      ar[i] = from[i];
    }
  }
  return to.concat(ar || Array.prototype.slice.call(from));
}
function __await(v) {
  return this instanceof __await ? (this.v = v, this) : new __await(v);
}
function __asyncGenerator(thisArg, _arguments, generator) {
  if (!Symbol.asyncIterator) throw new TypeError("Symbol.asyncIterator is not defined.");
  var g = generator.apply(thisArg, _arguments || []), i, q = [];
  return i = Object.create((typeof AsyncIterator === "function" ? AsyncIterator : Object).prototype), verb("next"), verb("throw"), verb("return", awaitReturn), i[Symbol.asyncIterator] = function() {
    return this;
  }, i;
  function awaitReturn(f) {
    return function(v) {
      return Promise.resolve(v).then(f, reject);
    };
  }
  function verb(n, f) {
    if (g[n]) {
      i[n] = function(v) {
        return new Promise(function(a, b) {
          q.push([n, v, a, b]) > 1 || resume(n, v);
        });
      };
      if (f) i[n] = f(i[n]);
    }
  }
  function resume(n, v) {
    try {
      step(g[n](v));
    } catch (e) {
      settle(q[0][3], e);
    }
  }
  function step(r) {
    r.value instanceof __await ? Promise.resolve(r.value.v).then(fulfill, reject) : settle(q[0][2], r);
  }
  function fulfill(value) {
    resume("next", value);
  }
  function reject(value) {
    resume("throw", value);
  }
  function settle(f, v) {
    if (f(v), q.shift(), q.length) resume(q[0][0], q[0][1]);
  }
}
function __asyncDelegator(o) {
  var i, p;
  return i = {}, verb("next"), verb("throw", function(e) {
    throw e;
  }), verb("return"), i[Symbol.iterator] = function() {
    return this;
  }, i;
  function verb(n, f) {
    i[n] = o[n] ? function(v) {
      return (p = !p) ? { value: __await(o[n](v)), done: false } : f ? f(v) : v;
    } : f;
  }
}
function __asyncValues(o) {
  if (!Symbol.asyncIterator) throw new TypeError("Symbol.asyncIterator is not defined.");
  var m = o[Symbol.asyncIterator], i;
  return m ? m.call(o) : (o = typeof __values === "function" ? __values(o) : o[Symbol.iterator](), i = {}, verb("next"), verb("throw"), verb("return"), i[Symbol.asyncIterator] = function() {
    return this;
  }, i);
  function verb(n) {
    i[n] = o[n] && function(v) {
      return new Promise(function(resolve, reject) {
        v = o[n](v), settle(resolve, reject, v.done, v.value);
      });
    };
  }
  function settle(resolve, reject, d, v) {
    Promise.resolve(v).then(function(v2) {
      resolve({ value: v2, done: d });
    }, reject);
  }
}
function __makeTemplateObject(cooked, raw) {
  if (Object.defineProperty) {
    Object.defineProperty(cooked, "raw", { value: raw });
  } else {
    cooked.raw = raw;
  }
  return cooked;
}
function __importStar(mod) {
  if (mod && mod.__esModule) return mod;
  var result = {};
  if (mod != null) {
    for (var k = ownKeys(mod), i = 0; i < k.length; i++) if (k[i] !== "default") __createBinding(result, mod, k[i]);
  }
  __setModuleDefault(result, mod);
  return result;
}
function __importDefault(mod) {
  return mod && mod.__esModule ? mod : { default: mod };
}
function __classPrivateFieldGet(receiver, state, kind, f) {
  if (kind === "a" && !f) throw new TypeError("Private accessor was defined without a getter");
  if (typeof state === "function" ? receiver !== state || !f : !state.has(receiver)) throw new TypeError("Cannot read private member from an object whose class did not declare it");
  return kind === "m" ? f : kind === "a" ? f.call(receiver) : f ? f.value : state.get(receiver);
}
function __classPrivateFieldSet(receiver, state, value, kind, f) {
  if (kind === "m") throw new TypeError("Private method is not writable");
  if (kind === "a" && !f) throw new TypeError("Private accessor was defined without a setter");
  if (typeof state === "function" ? receiver !== state || !f : !state.has(receiver)) throw new TypeError("Cannot write private member to an object whose class did not declare it");
  return kind === "a" ? f.call(receiver, value) : f ? f.value = value : state.set(receiver, value), value;
}
function __classPrivateFieldIn(state, receiver) {
  if (receiver === null || typeof receiver !== "object" && typeof receiver !== "function") throw new TypeError("Cannot use 'in' operator on non-object");
  return typeof state === "function" ? receiver === state : state.has(receiver);
}
function __addDisposableResource(env, value, async) {
  if (value !== null && value !== void 0) {
    if (typeof value !== "object" && typeof value !== "function") throw new TypeError("Object expected.");
    var dispose, inner;
    if (async) {
      if (!Symbol.asyncDispose) throw new TypeError("Symbol.asyncDispose is not defined.");
      dispose = value[Symbol.asyncDispose];
    }
    if (dispose === void 0) {
      if (!Symbol.dispose) throw new TypeError("Symbol.dispose is not defined.");
      dispose = value[Symbol.dispose];
      if (async) inner = dispose;
    }
    if (typeof dispose !== "function") throw new TypeError("Object not disposable.");
    if (inner) dispose = function() {
      try {
        inner.call(this);
      } catch (e) {
        return Promise.reject(e);
      }
    };
    env.stack.push({ value, dispose, async });
  } else if (async) {
    env.stack.push({ async: true });
  }
  return value;
}
function __disposeResources(env) {
  function fail(e) {
    env.error = env.hasError ? new _SuppressedError(e, env.error, "An error was suppressed during disposal.") : e;
    env.hasError = true;
  }
  var r, s = 0;
  function next() {
    while (r = env.stack.pop()) {
      try {
        if (!r.async && s === 1) return s = 0, env.stack.push(r), Promise.resolve().then(next);
        if (r.dispose) {
          var result = r.dispose.call(r.value);
          if (r.async) return s |= 2, Promise.resolve(result).then(next, function(e) {
            fail(e);
            return next();
          });
        } else s |= 1;
      } catch (e) {
        fail(e);
      }
    }
    if (s === 1) return env.hasError ? Promise.reject(env.error) : Promise.resolve();
    if (env.hasError) throw env.error;
  }
  return next();
}
function __rewriteRelativeImportExtension(path, preserveJsx) {
  if (typeof path === "string" && /^\.\.?\//.test(path)) {
    return path.replace(/\.(tsx)$|((?:\.d)?)((?:\.[^./]+?)?)\.([cm]?)ts$/i, function(m, tsx, d, ext, cm) {
      return tsx ? preserveJsx ? ".jsx" : ".js" : d && (!ext || !cm) ? m : d + ext + "." + cm.toLowerCase() + "js";
    });
  }
  return path;
}
var extendStatics, __assign, __createBinding, __setModuleDefault, ownKeys, _SuppressedError, tslib_es6_default;
var init_tslib_es6 = __esm({
  "../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/tslib/tslib.es6.mjs"() {
    extendStatics = function(d, b) {
      extendStatics = Object.setPrototypeOf || { __proto__: [] } instanceof Array && function(d2, b2) {
        d2.__proto__ = b2;
      } || function(d2, b2) {
        for (var p in b2) if (Object.prototype.hasOwnProperty.call(b2, p)) d2[p] = b2[p];
      };
      return extendStatics(d, b);
    };
    __assign = function() {
      __assign = Object.assign || function __assign2(t) {
        for (var s, i = 1, n = arguments.length; i < n; i++) {
          s = arguments[i];
          for (var p in s) if (Object.prototype.hasOwnProperty.call(s, p)) t[p] = s[p];
        }
        return t;
      };
      return __assign.apply(this, arguments);
    };
    __createBinding = Object.create ? (function(o, m, k, k2) {
      if (k2 === void 0) k2 = k;
      var desc = Object.getOwnPropertyDescriptor(m, k);
      if (!desc || ("get" in desc ? !m.__esModule : desc.writable || desc.configurable)) {
        desc = { enumerable: true, get: function() {
          return m[k];
        } };
      }
      Object.defineProperty(o, k2, desc);
    }) : (function(o, m, k, k2) {
      if (k2 === void 0) k2 = k;
      o[k2] = m[k];
    });
    __setModuleDefault = Object.create ? (function(o, v) {
      Object.defineProperty(o, "default", { enumerable: true, value: v });
    }) : function(o, v) {
      o["default"] = v;
    };
    ownKeys = function(o) {
      ownKeys = Object.getOwnPropertyNames || function(o2) {
        var ar = [];
        for (var k in o2) if (Object.prototype.hasOwnProperty.call(o2, k)) ar[ar.length] = k;
        return ar;
      };
      return ownKeys(o);
    };
    _SuppressedError = typeof SuppressedError === "function" ? SuppressedError : function(error, suppressed, message) {
      var e = new Error(message);
      return e.name = "SuppressedError", e.error = error, e.suppressed = suppressed, e;
    };
    tslib_es6_default = {
      __extends,
      __assign,
      __rest,
      __decorate,
      __param,
      __esDecorate,
      __runInitializers,
      __propKey,
      __setFunctionName,
      __metadata,
      __awaiter,
      __generator,
      __createBinding,
      __exportStar,
      __values,
      __read,
      __spread,
      __spreadArrays,
      __spreadArray,
      __await,
      __asyncGenerator,
      __asyncDelegator,
      __asyncValues,
      __makeTemplateObject,
      __importStar,
      __importDefault,
      __classPrivateFieldGet,
      __classPrivateFieldSet,
      __classPrivateFieldIn,
      __addDisposableResource,
      __disposeResources,
      __rewriteRelativeImportExtension
    };
  }
});

// ../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/@forge/auth/out/api.js
var require_api = __commonJS({
  "../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/@forge/auth/out/api.js"(exports2) {
    "use strict";
    Object.defineProperty(exports2, "__esModule", { value: true });
    exports2.createApiMethods = void 0;
    var fromEntries = (array) => {
      return array.reduce((acc, [key, value]) => {
        acc[key] = value;
        return acc;
      }, {});
    };
    var createApiMethods = (methodToPermissionMap, permissionCheckFactory) => {
      const apiMethodEntries = Object.entries(methodToPermissionMap).map(([methodName, permission]) => [methodName, permissionCheckFactory(permission)]);
      return fromEntries(apiMethodEntries);
    };
    exports2.createApiMethods = createApiMethods;
  }
});

// ../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/@forge/auth/out/confluence/permissions.js
var require_permissions = __commonJS({
  "../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/@forge/auth/out/confluence/permissions.js"(exports2) {
    "use strict";
    Object.defineProperty(exports2, "__esModule", { value: true });
    var API_PERMISSIONS_MAP = {
      canRead: "read",
      canUpdate: "update",
      canDelete: "delete"
    };
    exports2.default = API_PERMISSIONS_MAP;
  }
});

// ../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/@forge/auth/out/confluence/index.js
var require_confluence = __commonJS({
  "../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/@forge/auth/out/confluence/index.js"(exports2) {
    "use strict";
    Object.defineProperty(exports2, "__esModule", { value: true });
    exports2.authorizeConfluenceWithFetch = void 0;
    var tslib_1 = (init_tslib_es6(), __toCommonJS(tslib_es6_exports));
    var api_1 = require_api();
    var permissions_1 = tslib_1.__importDefault(require_permissions());
    var checkConfluencePermissions = async (requestConfluence, accountId, contentId, permission) => {
      const res = await requestConfluence(`/rest/api/content/${contentId}/permission/check`, {
        method: "post",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({
          subject: {
            type: "user",
            identifier: accountId
          },
          operation: permission
        })
      });
      return res;
    };
    var getPermissionsCheckFactory = (requestConfluence, accountId, contentId) => (permission) => {
      return async () => {
        const res = await checkConfluencePermissions(requestConfluence, accountId, contentId, permission);
        return Boolean(res?.hasPermission);
      };
    };
    var authorizeConfluenceWithFetch = (requestConfluence, accountId) => {
      return {
        onConfluenceContent: (contentId) => (0, api_1.createApiMethods)(permissions_1.default, getPermissionsCheckFactory(requestConfluence, accountId, contentId))
      };
    };
    exports2.authorizeConfluenceWithFetch = authorizeConfluenceWithFetch;
  }
});

// ../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/@forge/auth/out/jira/permissions.js
var require_permissions2 = __commonJS({
  "../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/@forge/auth/out/jira/permissions.js"(exports2) {
    "use strict";
    Object.defineProperty(exports2, "__esModule", { value: true });
    exports2.API_PROJECTS_PERMISSIONS_MAP = exports2.API_ISSUES_PERMISSIONS_MAP = void 0;
    var API_ISSUES_PERMISSIONS_MAP = {
      canAssign: "ASSIGN_ISSUES",
      canCreate: "CREATE_ISSUES",
      canEdit: "EDIT_ISSUES",
      canMove: "MOVE_ISSUES",
      canDelete: "DELETE_ISSUES",
      canAddComments: "ADD_COMMENTS",
      canEditAllComments: "EDIT_ALL_COMMENTS",
      canDeleteAllComments: "DELETE_ALL_COMMENTS",
      canCreateAttachments: "CREATE_ATTACHMENTS",
      canDeleteAllAttachments: "DELETE_ALL_ATTACHMENTS"
    };
    exports2.API_ISSUES_PERMISSIONS_MAP = API_ISSUES_PERMISSIONS_MAP;
    var API_PROJECTS_PERMISSIONS_MAP = {
      canAssignIssues: "ASSIGN_ISSUES",
      canCreateIssues: "CREATE_ISSUES",
      canEditIssues: "EDIT_ISSUES",
      canMoveIssues: "MOVE_ISSUES",
      canDeleteIssues: "DELETE_ISSUES",
      canAddComments: "ADD_COMMENTS",
      canEditAllComments: "EDIT_ALL_COMMENTS",
      canDeleteAllComments: "DELETE_ALL_COMMENTS",
      canCreateAttachments: "CREATE_ATTACHMENTS",
      canDeleteAllAttachments: "DELETE_ALL_ATTACHMENTS"
    };
    exports2.API_PROJECTS_PERMISSIONS_MAP = API_PROJECTS_PERMISSIONS_MAP;
  }
});

// ../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/@forge/auth/out/jira/index.js
var require_jira = __commonJS({
  "../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/@forge/auth/out/jira/index.js"(exports2) {
    "use strict";
    Object.defineProperty(exports2, "__esModule", { value: true });
    exports2.authorizeJiraWithFetch = void 0;
    var api_1 = require_api();
    var permissions_1 = require_permissions2();
    var arrayEquals = (a, b) => {
      return JSON.stringify(Array.from(a.map(String)).sort()) === JSON.stringify(Array.from(b.map(String)).sort());
    };
    var checkJiraPermissions = async (requestJira, accountId, projectPermissions) => {
      const res = await requestJira("/rest/api/3/permissions/check", {
        method: "post",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({
          accountId,
          projectPermissions
        })
      });
      return res;
    };
    var hasPermissionsForEntities = (projectPermissions, permission, type, entities) => {
      if (!entities || entities.length === 0)
        return true;
      const allowedEntities = projectPermissions.find((permissionResponse) => permissionResponse.permission === permission)?.[type];
      return !!allowedEntities && arrayEquals(allowedEntities, entities);
    };
    var getPermissionCheckFactory = (requestJira, accountId, type, entities) => (permission) => {
      return async () => {
        const { projectPermissions } = await checkJiraPermissions(requestJira, accountId, [
          {
            permissions: [permission],
            [type]: entities
          }
        ]);
        return hasPermissionsForEntities(projectPermissions, permission, type, entities);
      };
    };
    var toArray = (id) => Array.isArray(id) ? id : [id];
    var authorizeJiraWithFetch = (requestJira, accountId) => {
      return {
        onJira: async (projectPermissionsInput) => {
          const result = await checkJiraPermissions(requestJira, accountId, projectPermissionsInput);
          return result.projectPermissions || [];
        },
        onJiraProject: (projects) => (0, api_1.createApiMethods)(permissions_1.API_PROJECTS_PERMISSIONS_MAP, getPermissionCheckFactory(requestJira, accountId, "projects", toArray(projects))),
        onJiraIssue: (issues) => (0, api_1.createApiMethods)(permissions_1.API_ISSUES_PERMISSIONS_MAP, getPermissionCheckFactory(requestJira, accountId, "issues", toArray(issues)))
      };
    };
    exports2.authorizeJiraWithFetch = authorizeJiraWithFetch;
  }
});

// ../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/@forge/auth/out/index.js
var require_out = __commonJS({
  "../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/@forge/auth/out/index.js"(exports2) {
    "use strict";
    Object.defineProperty(exports2, "__esModule", { value: true });
    exports2.authorizeJiraWithFetch = exports2.authorizeConfluenceWithFetch = void 0;
    var confluence_1 = require_confluence();
    Object.defineProperty(exports2, "authorizeConfluenceWithFetch", { enumerable: true, get: function() {
      return confluence_1.authorizeConfluenceWithFetch;
    } });
    var jira_1 = require_jira();
    Object.defineProperty(exports2, "authorizeJiraWithFetch", { enumerable: true, get: function() {
      return jira_1.authorizeJiraWithFetch;
    } });
  }
});

// ../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/@forge/api/out/authorization/index.js
var require_authorization = __commonJS({
  "../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/@forge/api/out/authorization/index.js"(exports2) {
    "use strict";
    Object.defineProperty(exports2, "__esModule", { value: true });
    exports2.authorize = void 0;
    var auth_1 = require_out();
    var __1 = require_out4();
    var authorize = () => {
      const accountId = (0, __1.__getRuntime)().aaid;
      if (!accountId) {
        throw new Error(`Couldn\u2019t find the accountId of the invoking user. This API can only be used inside user-invoked modules.`);
      }
      return {
        ...(0, auth_1.authorizeConfluenceWithFetch)(async (path, opts) => {
          const res = await (0, __1.asUser)().requestConfluence((0, __1.assumeTrustedRoute)(path), opts);
          return res.json();
        }, accountId),
        ...(0, auth_1.authorizeJiraWithFetch)(async (path, opts) => {
          const res = await (0, __1.asUser)().requestJira((0, __1.assumeTrustedRoute)(path), opts);
          return res.json();
        }, accountId)
      };
    };
    exports2.authorize = authorize;
  }
});

// ../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/@forge/api/out/privacy/index.js
var require_privacy = __commonJS({
  "../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/@forge/api/out/privacy/index.js"(exports2) {
    "use strict";
    Object.defineProperty(exports2, "__esModule", { value: true });
    exports2.createReportPersonalData = exports2.LIMIT = exports2.URL = void 0;
    exports2.URL = "/app/report-accounts";
    exports2.LIMIT = 90;
    var createReportPersonalData = (requestAtlassian) => {
      return function fetchUpdates(accounts) {
        if (accounts.length === 0) {
          return Promise.resolve([]);
        }
        const request = requestAtlassian(exports2.URL, {
          method: "POST",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({ accounts: accounts.slice(0, exports2.LIMIT) })
        }).then(async (resp) => {
          if (resp.status === 200) {
            return (await resp.json()).accounts;
          }
          if (resp.status === 204) {
            return [];
          }
          return Promise.reject(resp);
        });
        return Promise.all([request, fetchUpdates(accounts.slice(exports2.LIMIT))]).then(([first, second]) => first.concat(second));
      };
    };
    exports2.createReportPersonalData = createReportPersonalData;
  }
});

// ../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/@forge/api/out/webTrigger.js
var require_webTrigger = __commonJS({
  "../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/@forge/api/out/webTrigger.js"(exports2) {
    "use strict";
    Object.defineProperty(exports2, "__esModule", { value: true });
    exports2.webTrigger = void 0;
    var runtime_1 = require_runtime();
    var fetch_1 = require_fetch();
    var proxyGetWebTriggerURL = (0, runtime_1.wrapInMetrics)("api.getWebTriggerUrl", async (webTriggerModuleKey, forceCreate, secretKeyConfig) => {
      const runtime = (0, runtime_1.__getRuntime)();
      const input = {
        appId: runtime.appContext.appId,
        envId: runtime.appContext.environmentId,
        triggerKey: webTriggerModuleKey,
        contextId: runtime.contextAri,
        ...secretKeyConfig !== void 0 ? {
          secretKeyConfig: {
            key: secretKeyConfig.key,
            ...secretKeyConfig.noExpiry !== void 0 ? { noExpiry: secretKeyConfig.noExpiry } : {}
          }
        } : {}
      };
      const response = await (0, fetch_1.__requestAtlassianAsApp)("/graphql", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          query: `
            mutation forge_app_createWebTriggerUrl($input: WebTriggerUrlInput!, $forceCreate: Boolean) {
              createWebTriggerUrl(input: $input, forceCreate: $forceCreate) {
                url
              }
            }
          `,
          variables: {
            input,
            forceCreate
          }
        })
      });
      if (!response.ok) {
        throw new Error(`Internal error occurred: Failed to get web trigger URL: ${response.statusText}.`);
      }
      const responseBody = await response.json();
      if (!responseBody?.data?.createWebTriggerUrl?.url) {
        throw new Error(`Internal error occurred: Failed to get web trigger URL.`);
      }
      return responseBody.data.createWebTriggerUrl.url;
    });
    var proxyDeleteWebTriggerURL = (0, runtime_1.wrapInMetrics)("api.deleteWebTriggerUrl", async (webTriggerUrl) => {
      const webTriggerUrlIdRegexGroupMatch = /\/(?:x1|public)\/([^\/\?\#]+)/;
      const matches = webTriggerUrl.match(webTriggerUrlIdRegexGroupMatch);
      if (!matches || matches.length < 2) {
        throw new Error("Internal error occurred: Failed to parse web trigger URL for ID");
      }
      const id = matches[1];
      const response = await (0, fetch_1.__requestAtlassianAsApp)("/graphql", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          query: `
            mutation forge_app_deleteWebTriggerUrl($id: ID!) {
              deleteWebTriggerUrl(id: $id) {
                success
                message
              }
            }
          `,
          variables: {
            id
          }
        })
      });
      if (!response.ok) {
        throw new Error(`Internal error occurred: Failed to delete web trigger URL: ${response.statusText}.`);
      }
      const responseBody = await response.json();
      if (!responseBody?.data?.deleteWebTriggerUrl?.success) {
        const errorText = responseBody?.data?.deleteWebTriggerUrl?.message || "unknown error";
        throw new Error(`Internal error occurred: Failed to delete web trigger URL: ${errorText}`);
      }
    });
    var proxyQueryWebTriggerURLs = (0, runtime_1.wrapInMetrics)("api.queryWebTriggerUrls", async (moduleKey) => {
      const runtime = (0, runtime_1.__getRuntime)();
      const response = await (0, fetch_1.__requestAtlassianAsApp)("/graphql", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          query: `
            query forge_app_webTriggerUrlsByAppContext($appId: ID!, $envId: ID!, $contextId: ID!) {
              webTriggerUrlsByAppContext(appId: $appId, envId: $envId, contextId: $contextId) {
                triggerKey
                url
              }
            }
          `,
          variables: {
            appId: runtime.appContext.appId,
            envId: runtime.appContext.environmentId,
            contextId: runtime.contextAri
          }
        })
      });
      if (!response.ok) {
        throw new Error(`Internal error occurred: Failed to get web trigger URLs: ${response.statusText}.`);
      }
      const responseBody = await response.json();
      if (!responseBody?.data?.webTriggerUrlsByAppContext) {
        throw new Error("Internal error occurred: No data from web trigger URLs query.");
      }
      let result = responseBody.data.webTriggerUrlsByAppContext;
      if (moduleKey) {
        result = result.filter((webTriggerResult) => webTriggerResult.triggerKey === moduleKey);
      }
      result = result.map((webTriggerResult) => ({
        moduleKey: webTriggerResult.triggerKey,
        url: webTriggerResult.url
      }));
      return result;
    });
    exports2.webTrigger = {
      getUrl: async (webTriggerModuleKey, optionsOrForceCreate) => {
        const forceCreate = typeof optionsOrForceCreate === "boolean" ? optionsOrForceCreate : optionsOrForceCreate?.forceCreate ?? false;
        const secretKeyConfig = typeof optionsOrForceCreate === "object" && optionsOrForceCreate !== null ? optionsOrForceCreate.secretKeyConfig : void 0;
        return proxyGetWebTriggerURL(webTriggerModuleKey, forceCreate, secretKeyConfig);
      },
      deleteUrl: async (webTriggerUrl) => proxyDeleteWebTriggerURL(webTriggerUrl),
      queryUrls: async (moduleKey) => proxyQueryWebTriggerURLs(moduleKey)
    };
  }
});

// ../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/@forge/i18n/out/constants.js
var require_constants = __commonJS({
  "../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/@forge/i18n/out/constants.js"(exports2) {
    "use strict";
    Object.defineProperty(exports2, "__esModule", { value: true });
    exports2.FORGE_SUPPORTED_LOCALE_CODES = exports2.I18N_BUNDLE_FOLDER_NAME = exports2.I18N_INFO_FILE_NAME = void 0;
    exports2.I18N_INFO_FILE_NAME = "i18n-info.json";
    exports2.I18N_BUNDLE_FOLDER_NAME = "__LOCALES__";
    exports2.FORGE_SUPPORTED_LOCALE_CODES = [
      "zh-CN",
      "zh-TW",
      "cs-CZ",
      "da-DK",
      "nl-NL",
      "en-US",
      "en-GB",
      "et-EE",
      "fi-FI",
      "fr-FR",
      "de-DE",
      "hu-HU",
      "is-IS",
      "it-IT",
      "ja-JP",
      "ko-KR",
      "no-NO",
      "pl-PL",
      "pt-BR",
      "pt-PT",
      "ro-RO",
      "ru-RU",
      "sk-SK",
      "tr-TR",
      "es-ES",
      "sv-SE"
    ];
  }
});

// ../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/@forge/i18n/out/translationsGetter.js
var require_translationsGetter = __commonJS({
  "../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/@forge/i18n/out/translationsGetter.js"(exports2) {
    "use strict";
    Object.defineProperty(exports2, "__esModule", { value: true });
    exports2.TranslationsGetter = exports2.TranslationGetterError = void 0;
    var pushIfNotExists = (array, item) => {
      if (!array.includes(item)) {
        array.push(item);
      }
    };
    var TranslationGetterError = class extends Error {
      constructor(message) {
        super(message);
        this.name = "TranslationGetterError";
      }
    };
    exports2.TranslationGetterError = TranslationGetterError;
    var TranslationsGetter = class {
      resourcesAccessor;
      i18nInfoConfig = null;
      translationResources = /* @__PURE__ */ new Map();
      constructor(resourcesAccessor) {
        this.resourcesAccessor = resourcesAccessor;
      }
      async getTranslations(locale, options = { fallback: true }) {
        const i18nInfoConfig = await this.getI18nInfoConfig();
        const { fallback } = options;
        if (!fallback) {
          let translationResource;
          if (i18nInfoConfig.locales.includes(locale)) {
            translationResource = await this.getTranslationResource(locale);
          }
          return {
            translations: translationResource ?? null,
            locale
          };
        }
        for (const targetLocale of this.getLocaleLookupOrder(locale, i18nInfoConfig)) {
          const translationResource = await this.getTranslationResource(targetLocale);
          if (translationResource) {
            return {
              translations: translationResource,
              locale: targetLocale
            };
          }
        }
        return {
          translations: null,
          locale
        };
      }
      async getTranslationsByLocaleLookupOrder(locale) {
        const i18nInfoConfig = await this.getI18nInfoConfig();
        const lookupOrder = this.getLocaleLookupOrder(locale, i18nInfoConfig);
        return await Promise.all(lookupOrder.map(async (targetLocale) => {
          const translationResource = await this.getTranslationResource(targetLocale);
          return {
            locale: targetLocale,
            translations: translationResource
          };
        }));
      }
      reset() {
        this.i18nInfoConfig = null;
        this.translationResources.clear();
      }
      async getTranslationResource(locale) {
        let resource = this.translationResources.get(locale);
        if (!resource) {
          try {
            resource = await this.resourcesAccessor.getTranslationResource(locale);
            this.translationResources.set(locale, resource);
          } catch (error) {
            if (error instanceof TranslationGetterError) {
              throw error;
            }
            throw new TranslationGetterError(`Failed to get translation resource for locale: ${locale}`);
          }
        }
        return resource;
      }
      async getI18nInfoConfig() {
        if (!this.i18nInfoConfig) {
          try {
            this.i18nInfoConfig = await this.resourcesAccessor.getI18nInfoConfig();
          } catch (error) {
            if (error instanceof TranslationGetterError) {
              throw error;
            }
            throw new TranslationGetterError("Failed to get i18n info config");
          }
        }
        return this.i18nInfoConfig;
      }
      getLocaleLookupOrder(locale, config) {
        const { locales, fallback } = config;
        const lookupOrder = [locale];
        const fallbackLocales = fallback[locale];
        if (fallbackLocales && Array.isArray(fallbackLocales) && fallbackLocales.length > 0) {
          lookupOrder.push(...fallbackLocales);
        }
        pushIfNotExists(lookupOrder, config.fallback.default);
        return lookupOrder.filter((locale2) => locales.includes(locale2));
      }
    };
    exports2.TranslationsGetter = TranslationsGetter;
  }
});

// ../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/lodash/isArray.js
var require_isArray = __commonJS({
  "../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/lodash/isArray.js"(exports2, module2) {
    var isArray = Array.isArray;
    module2.exports = isArray;
  }
});

// ../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/lodash/_freeGlobal.js
var require_freeGlobal = __commonJS({
  "../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/lodash/_freeGlobal.js"(exports2, module2) {
    var freeGlobal = typeof global == "object" && global && global.Object === Object && global;
    module2.exports = freeGlobal;
  }
});

// ../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/lodash/_root.js
var require_root = __commonJS({
  "../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/lodash/_root.js"(exports2, module2) {
    var freeGlobal = require_freeGlobal();
    var freeSelf = typeof self == "object" && self && self.Object === Object && self;
    var root = freeGlobal || freeSelf || Function("return this")();
    module2.exports = root;
  }
});

// ../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/lodash/_Symbol.js
var require_Symbol = __commonJS({
  "../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/lodash/_Symbol.js"(exports2, module2) {
    var root = require_root();
    var Symbol2 = root.Symbol;
    module2.exports = Symbol2;
  }
});

// ../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/lodash/_getRawTag.js
var require_getRawTag = __commonJS({
  "../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/lodash/_getRawTag.js"(exports2, module2) {
    var Symbol2 = require_Symbol();
    var objectProto = Object.prototype;
    var hasOwnProperty = objectProto.hasOwnProperty;
    var nativeObjectToString = objectProto.toString;
    var symToStringTag = Symbol2 ? Symbol2.toStringTag : void 0;
    function getRawTag(value) {
      var isOwn = hasOwnProperty.call(value, symToStringTag), tag = value[symToStringTag];
      try {
        value[symToStringTag] = void 0;
        var unmasked = true;
      } catch (e) {
      }
      var result = nativeObjectToString.call(value);
      if (unmasked) {
        if (isOwn) {
          value[symToStringTag] = tag;
        } else {
          delete value[symToStringTag];
        }
      }
      return result;
    }
    module2.exports = getRawTag;
  }
});

// ../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/lodash/_objectToString.js
var require_objectToString = __commonJS({
  "../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/lodash/_objectToString.js"(exports2, module2) {
    var objectProto = Object.prototype;
    var nativeObjectToString = objectProto.toString;
    function objectToString(value) {
      return nativeObjectToString.call(value);
    }
    module2.exports = objectToString;
  }
});

// ../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/lodash/_baseGetTag.js
var require_baseGetTag = __commonJS({
  "../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/lodash/_baseGetTag.js"(exports2, module2) {
    var Symbol2 = require_Symbol();
    var getRawTag = require_getRawTag();
    var objectToString = require_objectToString();
    var nullTag = "[object Null]";
    var undefinedTag = "[object Undefined]";
    var symToStringTag = Symbol2 ? Symbol2.toStringTag : void 0;
    function baseGetTag(value) {
      if (value == null) {
        return value === void 0 ? undefinedTag : nullTag;
      }
      return symToStringTag && symToStringTag in Object(value) ? getRawTag(value) : objectToString(value);
    }
    module2.exports = baseGetTag;
  }
});

// ../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/lodash/isObjectLike.js
var require_isObjectLike = __commonJS({
  "../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/lodash/isObjectLike.js"(exports2, module2) {
    function isObjectLike(value) {
      return value != null && typeof value == "object";
    }
    module2.exports = isObjectLike;
  }
});

// ../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/lodash/isSymbol.js
var require_isSymbol = __commonJS({
  "../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/lodash/isSymbol.js"(exports2, module2) {
    var baseGetTag = require_baseGetTag();
    var isObjectLike = require_isObjectLike();
    var symbolTag = "[object Symbol]";
    function isSymbol(value) {
      return typeof value == "symbol" || isObjectLike(value) && baseGetTag(value) == symbolTag;
    }
    module2.exports = isSymbol;
  }
});

// ../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/lodash/_isKey.js
var require_isKey = __commonJS({
  "../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/lodash/_isKey.js"(exports2, module2) {
    var isArray = require_isArray();
    var isSymbol = require_isSymbol();
    var reIsDeepProp = /\.|\[(?:[^[\]]*|(["'])(?:(?!\1)[^\\]|\\.)*?\1)\]/;
    var reIsPlainProp = /^\w*$/;
    function isKey(value, object) {
      if (isArray(value)) {
        return false;
      }
      var type = typeof value;
      if (type == "number" || type == "symbol" || type == "boolean" || value == null || isSymbol(value)) {
        return true;
      }
      return reIsPlainProp.test(value) || !reIsDeepProp.test(value) || object != null && value in Object(object);
    }
    module2.exports = isKey;
  }
});

// ../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/lodash/isObject.js
var require_isObject = __commonJS({
  "../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/lodash/isObject.js"(exports2, module2) {
    function isObject(value) {
      var type = typeof value;
      return value != null && (type == "object" || type == "function");
    }
    module2.exports = isObject;
  }
});

// ../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/lodash/isFunction.js
var require_isFunction = __commonJS({
  "../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/lodash/isFunction.js"(exports2, module2) {
    var baseGetTag = require_baseGetTag();
    var isObject = require_isObject();
    var asyncTag = "[object AsyncFunction]";
    var funcTag = "[object Function]";
    var genTag = "[object GeneratorFunction]";
    var proxyTag = "[object Proxy]";
    function isFunction(value) {
      if (!isObject(value)) {
        return false;
      }
      var tag = baseGetTag(value);
      return tag == funcTag || tag == genTag || tag == asyncTag || tag == proxyTag;
    }
    module2.exports = isFunction;
  }
});

// ../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/lodash/_coreJsData.js
var require_coreJsData = __commonJS({
  "../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/lodash/_coreJsData.js"(exports2, module2) {
    var root = require_root();
    var coreJsData = root["__core-js_shared__"];
    module2.exports = coreJsData;
  }
});

// ../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/lodash/_isMasked.js
var require_isMasked = __commonJS({
  "../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/lodash/_isMasked.js"(exports2, module2) {
    var coreJsData = require_coreJsData();
    var maskSrcKey = (function() {
      var uid = /[^.]+$/.exec(coreJsData && coreJsData.keys && coreJsData.keys.IE_PROTO || "");
      return uid ? "Symbol(src)_1." + uid : "";
    })();
    function isMasked(func) {
      return !!maskSrcKey && maskSrcKey in func;
    }
    module2.exports = isMasked;
  }
});

// ../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/lodash/_toSource.js
var require_toSource = __commonJS({
  "../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/lodash/_toSource.js"(exports2, module2) {
    var funcProto = Function.prototype;
    var funcToString = funcProto.toString;
    function toSource(func) {
      if (func != null) {
        try {
          return funcToString.call(func);
        } catch (e) {
        }
        try {
          return func + "";
        } catch (e) {
        }
      }
      return "";
    }
    module2.exports = toSource;
  }
});

// ../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/lodash/_baseIsNative.js
var require_baseIsNative = __commonJS({
  "../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/lodash/_baseIsNative.js"(exports2, module2) {
    var isFunction = require_isFunction();
    var isMasked = require_isMasked();
    var isObject = require_isObject();
    var toSource = require_toSource();
    var reRegExpChar = /[\\^$.*+?()[\]{}|]/g;
    var reIsHostCtor = /^\[object .+?Constructor\]$/;
    var funcProto = Function.prototype;
    var objectProto = Object.prototype;
    var funcToString = funcProto.toString;
    var hasOwnProperty = objectProto.hasOwnProperty;
    var reIsNative = RegExp(
      "^" + funcToString.call(hasOwnProperty).replace(reRegExpChar, "\\$&").replace(/hasOwnProperty|(function).*?(?=\\\()| for .+?(?=\\\])/g, "$1.*?") + "$"
    );
    function baseIsNative(value) {
      if (!isObject(value) || isMasked(value)) {
        return false;
      }
      var pattern = isFunction(value) ? reIsNative : reIsHostCtor;
      return pattern.test(toSource(value));
    }
    module2.exports = baseIsNative;
  }
});

// ../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/lodash/_getValue.js
var require_getValue = __commonJS({
  "../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/lodash/_getValue.js"(exports2, module2) {
    function getValue(object, key) {
      return object == null ? void 0 : object[key];
    }
    module2.exports = getValue;
  }
});

// ../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/lodash/_getNative.js
var require_getNative = __commonJS({
  "../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/lodash/_getNative.js"(exports2, module2) {
    var baseIsNative = require_baseIsNative();
    var getValue = require_getValue();
    function getNative(object, key) {
      var value = getValue(object, key);
      return baseIsNative(value) ? value : void 0;
    }
    module2.exports = getNative;
  }
});

// ../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/lodash/_nativeCreate.js
var require_nativeCreate = __commonJS({
  "../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/lodash/_nativeCreate.js"(exports2, module2) {
    var getNative = require_getNative();
    var nativeCreate = getNative(Object, "create");
    module2.exports = nativeCreate;
  }
});

// ../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/lodash/_hashClear.js
var require_hashClear = __commonJS({
  "../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/lodash/_hashClear.js"(exports2, module2) {
    var nativeCreate = require_nativeCreate();
    function hashClear() {
      this.__data__ = nativeCreate ? nativeCreate(null) : {};
      this.size = 0;
    }
    module2.exports = hashClear;
  }
});

// ../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/lodash/_hashDelete.js
var require_hashDelete = __commonJS({
  "../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/lodash/_hashDelete.js"(exports2, module2) {
    function hashDelete(key) {
      var result = this.has(key) && delete this.__data__[key];
      this.size -= result ? 1 : 0;
      return result;
    }
    module2.exports = hashDelete;
  }
});

// ../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/lodash/_hashGet.js
var require_hashGet = __commonJS({
  "../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/lodash/_hashGet.js"(exports2, module2) {
    var nativeCreate = require_nativeCreate();
    var HASH_UNDEFINED = "__lodash_hash_undefined__";
    var objectProto = Object.prototype;
    var hasOwnProperty = objectProto.hasOwnProperty;
    function hashGet(key) {
      var data = this.__data__;
      if (nativeCreate) {
        var result = data[key];
        return result === HASH_UNDEFINED ? void 0 : result;
      }
      return hasOwnProperty.call(data, key) ? data[key] : void 0;
    }
    module2.exports = hashGet;
  }
});

// ../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/lodash/_hashHas.js
var require_hashHas = __commonJS({
  "../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/lodash/_hashHas.js"(exports2, module2) {
    var nativeCreate = require_nativeCreate();
    var objectProto = Object.prototype;
    var hasOwnProperty = objectProto.hasOwnProperty;
    function hashHas(key) {
      var data = this.__data__;
      return nativeCreate ? data[key] !== void 0 : hasOwnProperty.call(data, key);
    }
    module2.exports = hashHas;
  }
});

// ../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/lodash/_hashSet.js
var require_hashSet = __commonJS({
  "../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/lodash/_hashSet.js"(exports2, module2) {
    var nativeCreate = require_nativeCreate();
    var HASH_UNDEFINED = "__lodash_hash_undefined__";
    function hashSet(key, value) {
      var data = this.__data__;
      this.size += this.has(key) ? 0 : 1;
      data[key] = nativeCreate && value === void 0 ? HASH_UNDEFINED : value;
      return this;
    }
    module2.exports = hashSet;
  }
});

// ../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/lodash/_Hash.js
var require_Hash = __commonJS({
  "../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/lodash/_Hash.js"(exports2, module2) {
    var hashClear = require_hashClear();
    var hashDelete = require_hashDelete();
    var hashGet = require_hashGet();
    var hashHas = require_hashHas();
    var hashSet = require_hashSet();
    function Hash(entries) {
      var index = -1, length = entries == null ? 0 : entries.length;
      this.clear();
      while (++index < length) {
        var entry = entries[index];
        this.set(entry[0], entry[1]);
      }
    }
    Hash.prototype.clear = hashClear;
    Hash.prototype["delete"] = hashDelete;
    Hash.prototype.get = hashGet;
    Hash.prototype.has = hashHas;
    Hash.prototype.set = hashSet;
    module2.exports = Hash;
  }
});

// ../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/lodash/_listCacheClear.js
var require_listCacheClear = __commonJS({
  "../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/lodash/_listCacheClear.js"(exports2, module2) {
    function listCacheClear() {
      this.__data__ = [];
      this.size = 0;
    }
    module2.exports = listCacheClear;
  }
});

// ../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/lodash/eq.js
var require_eq = __commonJS({
  "../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/lodash/eq.js"(exports2, module2) {
    function eq(value, other) {
      return value === other || value !== value && other !== other;
    }
    module2.exports = eq;
  }
});

// ../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/lodash/_assocIndexOf.js
var require_assocIndexOf = __commonJS({
  "../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/lodash/_assocIndexOf.js"(exports2, module2) {
    var eq = require_eq();
    function assocIndexOf(array, key) {
      var length = array.length;
      while (length--) {
        if (eq(array[length][0], key)) {
          return length;
        }
      }
      return -1;
    }
    module2.exports = assocIndexOf;
  }
});

// ../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/lodash/_listCacheDelete.js
var require_listCacheDelete = __commonJS({
  "../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/lodash/_listCacheDelete.js"(exports2, module2) {
    var assocIndexOf = require_assocIndexOf();
    var arrayProto = Array.prototype;
    var splice = arrayProto.splice;
    function listCacheDelete(key) {
      var data = this.__data__, index = assocIndexOf(data, key);
      if (index < 0) {
        return false;
      }
      var lastIndex = data.length - 1;
      if (index == lastIndex) {
        data.pop();
      } else {
        splice.call(data, index, 1);
      }
      --this.size;
      return true;
    }
    module2.exports = listCacheDelete;
  }
});

// ../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/lodash/_listCacheGet.js
var require_listCacheGet = __commonJS({
  "../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/lodash/_listCacheGet.js"(exports2, module2) {
    var assocIndexOf = require_assocIndexOf();
    function listCacheGet(key) {
      var data = this.__data__, index = assocIndexOf(data, key);
      return index < 0 ? void 0 : data[index][1];
    }
    module2.exports = listCacheGet;
  }
});

// ../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/lodash/_listCacheHas.js
var require_listCacheHas = __commonJS({
  "../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/lodash/_listCacheHas.js"(exports2, module2) {
    var assocIndexOf = require_assocIndexOf();
    function listCacheHas(key) {
      return assocIndexOf(this.__data__, key) > -1;
    }
    module2.exports = listCacheHas;
  }
});

// ../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/lodash/_listCacheSet.js
var require_listCacheSet = __commonJS({
  "../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/lodash/_listCacheSet.js"(exports2, module2) {
    var assocIndexOf = require_assocIndexOf();
    function listCacheSet(key, value) {
      var data = this.__data__, index = assocIndexOf(data, key);
      if (index < 0) {
        ++this.size;
        data.push([key, value]);
      } else {
        data[index][1] = value;
      }
      return this;
    }
    module2.exports = listCacheSet;
  }
});

// ../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/lodash/_ListCache.js
var require_ListCache = __commonJS({
  "../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/lodash/_ListCache.js"(exports2, module2) {
    var listCacheClear = require_listCacheClear();
    var listCacheDelete = require_listCacheDelete();
    var listCacheGet = require_listCacheGet();
    var listCacheHas = require_listCacheHas();
    var listCacheSet = require_listCacheSet();
    function ListCache(entries) {
      var index = -1, length = entries == null ? 0 : entries.length;
      this.clear();
      while (++index < length) {
        var entry = entries[index];
        this.set(entry[0], entry[1]);
      }
    }
    ListCache.prototype.clear = listCacheClear;
    ListCache.prototype["delete"] = listCacheDelete;
    ListCache.prototype.get = listCacheGet;
    ListCache.prototype.has = listCacheHas;
    ListCache.prototype.set = listCacheSet;
    module2.exports = ListCache;
  }
});

// ../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/lodash/_Map.js
var require_Map = __commonJS({
  "../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/lodash/_Map.js"(exports2, module2) {
    var getNative = require_getNative();
    var root = require_root();
    var Map2 = getNative(root, "Map");
    module2.exports = Map2;
  }
});

// ../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/lodash/_mapCacheClear.js
var require_mapCacheClear = __commonJS({
  "../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/lodash/_mapCacheClear.js"(exports2, module2) {
    var Hash = require_Hash();
    var ListCache = require_ListCache();
    var Map2 = require_Map();
    function mapCacheClear() {
      this.size = 0;
      this.__data__ = {
        "hash": new Hash(),
        "map": new (Map2 || ListCache)(),
        "string": new Hash()
      };
    }
    module2.exports = mapCacheClear;
  }
});

// ../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/lodash/_isKeyable.js
var require_isKeyable = __commonJS({
  "../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/lodash/_isKeyable.js"(exports2, module2) {
    function isKeyable(value) {
      var type = typeof value;
      return type == "string" || type == "number" || type == "symbol" || type == "boolean" ? value !== "__proto__" : value === null;
    }
    module2.exports = isKeyable;
  }
});

// ../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/lodash/_getMapData.js
var require_getMapData = __commonJS({
  "../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/lodash/_getMapData.js"(exports2, module2) {
    var isKeyable = require_isKeyable();
    function getMapData(map, key) {
      var data = map.__data__;
      return isKeyable(key) ? data[typeof key == "string" ? "string" : "hash"] : data.map;
    }
    module2.exports = getMapData;
  }
});

// ../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/lodash/_mapCacheDelete.js
var require_mapCacheDelete = __commonJS({
  "../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/lodash/_mapCacheDelete.js"(exports2, module2) {
    var getMapData = require_getMapData();
    function mapCacheDelete(key) {
      var result = getMapData(this, key)["delete"](key);
      this.size -= result ? 1 : 0;
      return result;
    }
    module2.exports = mapCacheDelete;
  }
});

// ../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/lodash/_mapCacheGet.js
var require_mapCacheGet = __commonJS({
  "../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/lodash/_mapCacheGet.js"(exports2, module2) {
    var getMapData = require_getMapData();
    function mapCacheGet(key) {
      return getMapData(this, key).get(key);
    }
    module2.exports = mapCacheGet;
  }
});

// ../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/lodash/_mapCacheHas.js
var require_mapCacheHas = __commonJS({
  "../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/lodash/_mapCacheHas.js"(exports2, module2) {
    var getMapData = require_getMapData();
    function mapCacheHas(key) {
      return getMapData(this, key).has(key);
    }
    module2.exports = mapCacheHas;
  }
});

// ../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/lodash/_mapCacheSet.js
var require_mapCacheSet = __commonJS({
  "../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/lodash/_mapCacheSet.js"(exports2, module2) {
    var getMapData = require_getMapData();
    function mapCacheSet(key, value) {
      var data = getMapData(this, key), size = data.size;
      data.set(key, value);
      this.size += data.size == size ? 0 : 1;
      return this;
    }
    module2.exports = mapCacheSet;
  }
});

// ../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/lodash/_MapCache.js
var require_MapCache = __commonJS({
  "../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/lodash/_MapCache.js"(exports2, module2) {
    var mapCacheClear = require_mapCacheClear();
    var mapCacheDelete = require_mapCacheDelete();
    var mapCacheGet = require_mapCacheGet();
    var mapCacheHas = require_mapCacheHas();
    var mapCacheSet = require_mapCacheSet();
    function MapCache(entries) {
      var index = -1, length = entries == null ? 0 : entries.length;
      this.clear();
      while (++index < length) {
        var entry = entries[index];
        this.set(entry[0], entry[1]);
      }
    }
    MapCache.prototype.clear = mapCacheClear;
    MapCache.prototype["delete"] = mapCacheDelete;
    MapCache.prototype.get = mapCacheGet;
    MapCache.prototype.has = mapCacheHas;
    MapCache.prototype.set = mapCacheSet;
    module2.exports = MapCache;
  }
});

// ../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/lodash/memoize.js
var require_memoize = __commonJS({
  "../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/lodash/memoize.js"(exports2, module2) {
    var MapCache = require_MapCache();
    var FUNC_ERROR_TEXT = "Expected a function";
    function memoize(func, resolver) {
      if (typeof func != "function" || resolver != null && typeof resolver != "function") {
        throw new TypeError(FUNC_ERROR_TEXT);
      }
      var memoized = function() {
        var args = arguments, key = resolver ? resolver.apply(this, args) : args[0], cache = memoized.cache;
        if (cache.has(key)) {
          return cache.get(key);
        }
        var result = func.apply(this, args);
        memoized.cache = cache.set(key, result) || cache;
        return result;
      };
      memoized.cache = new (memoize.Cache || MapCache)();
      return memoized;
    }
    memoize.Cache = MapCache;
    module2.exports = memoize;
  }
});

// ../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/lodash/_memoizeCapped.js
var require_memoizeCapped = __commonJS({
  "../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/lodash/_memoizeCapped.js"(exports2, module2) {
    var memoize = require_memoize();
    var MAX_MEMOIZE_SIZE = 500;
    function memoizeCapped(func) {
      var result = memoize(func, function(key) {
        if (cache.size === MAX_MEMOIZE_SIZE) {
          cache.clear();
        }
        return key;
      });
      var cache = result.cache;
      return result;
    }
    module2.exports = memoizeCapped;
  }
});

// ../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/lodash/_stringToPath.js
var require_stringToPath = __commonJS({
  "../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/lodash/_stringToPath.js"(exports2, module2) {
    var memoizeCapped = require_memoizeCapped();
    var rePropName = /[^.[\]]+|\[(?:(-?\d+(?:\.\d+)?)|(["'])((?:(?!\2)[^\\]|\\.)*?)\2)\]|(?=(?:\.|\[\])(?:\.|\[\]|$))/g;
    var reEscapeChar = /\\(\\)?/g;
    var stringToPath = memoizeCapped(function(string) {
      var result = [];
      if (string.charCodeAt(0) === 46) {
        result.push("");
      }
      string.replace(rePropName, function(match, number, quote, subString) {
        result.push(quote ? subString.replace(reEscapeChar, "$1") : number || match);
      });
      return result;
    });
    module2.exports = stringToPath;
  }
});

// ../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/lodash/_arrayMap.js
var require_arrayMap = __commonJS({
  "../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/lodash/_arrayMap.js"(exports2, module2) {
    function arrayMap(array, iteratee) {
      var index = -1, length = array == null ? 0 : array.length, result = Array(length);
      while (++index < length) {
        result[index] = iteratee(array[index], index, array);
      }
      return result;
    }
    module2.exports = arrayMap;
  }
});

// ../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/lodash/_baseToString.js
var require_baseToString = __commonJS({
  "../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/lodash/_baseToString.js"(exports2, module2) {
    var Symbol2 = require_Symbol();
    var arrayMap = require_arrayMap();
    var isArray = require_isArray();
    var isSymbol = require_isSymbol();
    var INFINITY = 1 / 0;
    var symbolProto = Symbol2 ? Symbol2.prototype : void 0;
    var symbolToString = symbolProto ? symbolProto.toString : void 0;
    function baseToString(value) {
      if (typeof value == "string") {
        return value;
      }
      if (isArray(value)) {
        return arrayMap(value, baseToString) + "";
      }
      if (isSymbol(value)) {
        return symbolToString ? symbolToString.call(value) : "";
      }
      var result = value + "";
      return result == "0" && 1 / value == -INFINITY ? "-0" : result;
    }
    module2.exports = baseToString;
  }
});

// ../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/lodash/toString.js
var require_toString = __commonJS({
  "../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/lodash/toString.js"(exports2, module2) {
    var baseToString = require_baseToString();
    function toString(value) {
      return value == null ? "" : baseToString(value);
    }
    module2.exports = toString;
  }
});

// ../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/lodash/_castPath.js
var require_castPath = __commonJS({
  "../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/lodash/_castPath.js"(exports2, module2) {
    var isArray = require_isArray();
    var isKey = require_isKey();
    var stringToPath = require_stringToPath();
    var toString = require_toString();
    function castPath(value, object) {
      if (isArray(value)) {
        return value;
      }
      return isKey(value, object) ? [value] : stringToPath(toString(value));
    }
    module2.exports = castPath;
  }
});

// ../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/lodash/_toKey.js
var require_toKey = __commonJS({
  "../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/lodash/_toKey.js"(exports2, module2) {
    var isSymbol = require_isSymbol();
    var INFINITY = 1 / 0;
    function toKey(value) {
      if (typeof value == "string" || isSymbol(value)) {
        return value;
      }
      var result = value + "";
      return result == "0" && 1 / value == -INFINITY ? "-0" : result;
    }
    module2.exports = toKey;
  }
});

// ../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/lodash/_baseGet.js
var require_baseGet = __commonJS({
  "../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/lodash/_baseGet.js"(exports2, module2) {
    var castPath = require_castPath();
    var toKey = require_toKey();
    function baseGet(object, path) {
      path = castPath(path, object);
      var index = 0, length = path.length;
      while (object != null && index < length) {
        object = object[toKey(path[index++])];
      }
      return index && index == length ? object : void 0;
    }
    module2.exports = baseGet;
  }
});

// ../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/lodash/get.js
var require_get = __commonJS({
  "../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/lodash/get.js"(exports2, module2) {
    var baseGet = require_baseGet();
    function get(object, path, defaultValue) {
      var result = object == null ? void 0 : baseGet(object, path);
      return result === void 0 ? defaultValue : result;
    }
    module2.exports = get;
  }
});

// ../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/@forge/i18n/out/translationValueGetter.js
var require_translationValueGetter = __commonJS({
  "../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/@forge/i18n/out/translationValueGetter.js"(exports2) {
    "use strict";
    Object.defineProperty(exports2, "__esModule", { value: true });
    exports2.getTranslationValueFromContent = exports2.getTranslationValue = void 0;
    var tslib_1 = (init_tslib_es6(), __toCommonJS(tslib_es6_exports));
    var get_1 = tslib_1.__importDefault(require_get());
    var getTranslationValue = (translationLookup, i18nKey, locale) => {
      const translation = translationLookup[locale];
      if (!translation) {
        return null;
      }
      return (0, exports2.getTranslationValueFromContent)(translation, i18nKey);
    };
    exports2.getTranslationValue = getTranslationValue;
    var getTranslationValueFromContent = (translationContent, i18nKey) => {
      let translationValue = translationContent[i18nKey];
      if (!translationValue) {
        const keyTokens = i18nKey.split(".");
        if (keyTokens.length > 1) {
          translationValue = (0, get_1.default)(translationContent, keyTokens, null);
        }
      }
      return typeof translationValue === "string" ? translationValue : null;
    };
    exports2.getTranslationValueFromContent = getTranslationValueFromContent;
  }
});

// ../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/@forge/i18n/out/translator.js
var require_translator = __commonJS({
  "../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/@forge/i18n/out/translator.js"(exports2) {
    "use strict";
    Object.defineProperty(exports2, "__esModule", { value: true });
    exports2.Translator = void 0;
    var translationValueGetter_1 = require_translationValueGetter();
    var Translator = class {
      locale;
      translationsGetter;
      localeLookupOrderedTranslations = null;
      cache = /* @__PURE__ */ new Map();
      constructor(locale, translationsGetter) {
        this.locale = locale;
        this.translationsGetter = translationsGetter;
      }
      async init() {
        this.localeLookupOrderedTranslations = await this.translationsGetter.getTranslationsByLocaleLookupOrder(this.locale);
      }
      translate(i18nKey) {
        if (!this.localeLookupOrderedTranslations) {
          throw new Error("TranslationLookup not initialized");
        }
        let result = this.cache.get(i18nKey);
        if (result === void 0) {
          for (const { translations } of this.localeLookupOrderedTranslations) {
            const translationValue = (0, translationValueGetter_1.getTranslationValueFromContent)(translations, i18nKey);
            if (translationValue !== null) {
              result = translationValue;
              break;
            }
          }
          result = result ?? null;
          this.cache.set(i18nKey, result);
        }
        return result;
      }
    };
    exports2.Translator = Translator;
  }
});

// ../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/@forge/i18n/out/ensureLocale.js
var require_ensureLocale = __commonJS({
  "../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/@forge/i18n/out/ensureLocale.js"(exports2) {
    "use strict";
    Object.defineProperty(exports2, "__esModule", { value: true });
    exports2.ensureLocale = void 0;
    var constants_1 = require_constants();
    var forgeSupportedLocaleCodesSet = new Set(constants_1.FORGE_SUPPORTED_LOCALE_CODES);
    var localeFallbacks = {
      "en-UK": "en-GB",
      "nb-NO": "no-NO"
    };
    var languageToLocaleCodeMap = constants_1.FORGE_SUPPORTED_LOCALE_CODES.reduce((agg, code) => {
      const [lng] = code.split("-");
      if (!agg[lng]) {
        agg[lng] = code;
      }
      return agg;
    }, {
      nb: "no-NO",
      pt: "pt-PT"
    });
    var ensureLocale = (rawLocale) => {
      const locale = rawLocale.replace("_", "-");
      if (forgeSupportedLocaleCodesSet.has(locale)) {
        return locale;
      }
      return languageToLocaleCodeMap[locale] ?? localeFallbacks[locale] ?? null;
    };
    exports2.ensureLocale = ensureLocale;
  }
});

// ../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/@forge/i18n/out/moduleI18nHelper.js
var require_moduleI18nHelper = __commonJS({
  "../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/@forge/i18n/out/moduleI18nHelper.js"(exports2) {
    "use strict";
    Object.defineProperty(exports2, "__esModule", { value: true });
    exports2.extractI18nPropertiesFromModules = exports2.extractI18nKeysFromModules = exports2.getI18nSupportedModuleEntries = void 0;
    var isObject = (value) => {
      return typeof value === "object" && value !== null && !Array.isArray(value);
    };
    var isI18nValue = (value) => {
      return typeof value?.i18n === "string";
    };
    var isConnectModuleKey = (moduleKey) => moduleKey.startsWith("connect-");
    var isCoreModuleKey = (moduleKey) => moduleKey.startsWith("core:");
    var getI18nKeysFromObject = (obj) => {
      const visited = /* @__PURE__ */ new Set();
      const visit = (value, i18nPath) => {
        if (!isObject(value) || visited.has(value)) {
          return [];
        }
        visited.add(value);
        return Object.entries(value).flatMap(([propKey, propValue]) => {
          const currentPath = [...i18nPath, propKey];
          if (isI18nValue(propValue)) {
            return [{ propertyPath: currentPath, key: propValue.i18n }];
          } else if (Array.isArray(propValue)) {
            return propValue.flatMap((item) => visit(item, currentPath));
          }
          return visit(propValue, currentPath);
        });
      };
      return visit(obj, []);
    };
    var getI18nSupportedModuleEntries = (modules) => {
      return Object.entries(modules).flatMap(([moduleKey, moduleEntries]) => {
        if (!isConnectModuleKey(moduleKey) && !isCoreModuleKey(moduleKey) && moduleEntries && Array.isArray(moduleEntries) && moduleEntries.length > 0) {
          return moduleEntries.map((moduleEntry) => [moduleEntry, moduleKey]);
        }
        return [];
      });
    };
    exports2.getI18nSupportedModuleEntries = getI18nSupportedModuleEntries;
    var extractI18nKeysFromModules = (modules) => {
      const i18nKeys = /* @__PURE__ */ new Set();
      for (const moduleEntry of (0, exports2.getI18nSupportedModuleEntries)(modules)) {
        const i18nKeysForEntryValue = getI18nKeysFromObject(moduleEntry[0]);
        for (const { key } of i18nKeysForEntryValue) {
          i18nKeys.add(key);
        }
      }
      return i18nKeys.size > 0 ? Array.from(i18nKeys) : [];
    };
    exports2.extractI18nKeysFromModules = extractI18nKeysFromModules;
    var extractI18nPropertiesFromModules = (modules) => {
      const moduleI18nProperties = [];
      for (const moduleEntry of (0, exports2.getI18nSupportedModuleEntries)(modules)) {
        const i18nKeysForEntryValue = getI18nKeysFromObject(moduleEntry[0]);
        for (const i18nObj of i18nKeysForEntryValue) {
          moduleI18nProperties.push({ moduleName: moduleEntry[1], ...i18nObj });
        }
      }
      return moduleI18nProperties;
    };
    exports2.extractI18nPropertiesFromModules = extractI18nPropertiesFromModules;
  }
});

// ../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/@forge/i18n/out/types.js
var require_types = __commonJS({
  "../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/@forge/i18n/out/types.js"(exports2) {
    "use strict";
    Object.defineProperty(exports2, "__esModule", { value: true });
  }
});

// ../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/@forge/i18n/out/index.js
var require_out2 = __commonJS({
  "../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/@forge/i18n/out/index.js"(exports2) {
    "use strict";
    Object.defineProperty(exports2, "__esModule", { value: true });
    exports2.getI18nSupportedModuleEntries = exports2.extractI18nPropertiesFromModules = exports2.extractI18nKeysFromModules = exports2.getTranslationValue = void 0;
    var tslib_1 = (init_tslib_es6(), __toCommonJS(tslib_es6_exports));
    tslib_1.__exportStar(require_constants(), exports2);
    tslib_1.__exportStar(require_translationsGetter(), exports2);
    tslib_1.__exportStar(require_translator(), exports2);
    tslib_1.__exportStar(require_ensureLocale(), exports2);
    var translationValueGetter_1 = require_translationValueGetter();
    Object.defineProperty(exports2, "getTranslationValue", { enumerable: true, get: function() {
      return translationValueGetter_1.getTranslationValue;
    } });
    var moduleI18nHelper_1 = require_moduleI18nHelper();
    Object.defineProperty(exports2, "extractI18nKeysFromModules", { enumerable: true, get: function() {
      return moduleI18nHelper_1.extractI18nKeysFromModules;
    } });
    Object.defineProperty(exports2, "extractI18nPropertiesFromModules", { enumerable: true, get: function() {
      return moduleI18nHelper_1.extractI18nPropertiesFromModules;
    } });
    Object.defineProperty(exports2, "getI18nSupportedModuleEntries", { enumerable: true, get: function() {
      return moduleI18nHelper_1.getI18nSupportedModuleEntries;
    } });
    tslib_1.__exportStar(require_types(), exports2);
  }
});

// ../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/@forge/api/out/api/i18n.js
var require_i18n = __commonJS({
  "../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/@forge/api/out/api/i18n.js"(exports2) {
    "use strict";
    Object.defineProperty(exports2, "__esModule", { value: true });
    exports2.i18n = exports2.createTranslationFunction = exports2.getTranslations = exports2.resetTranslationsCache = void 0;
    var i18n_1 = require_out2();
    var fs_1 = require("fs");
    var path_1 = require("path");
    var runtime_1 = require_runtime();
    var getI18nBundleFolderPath = () => {
      const { appCodeDir } = (0, runtime_1.__getRuntime)().container;
      return appCodeDir ? [appCodeDir, i18n_1.I18N_BUNDLE_FOLDER_NAME] : [i18n_1.I18N_BUNDLE_FOLDER_NAME];
    };
    var readLocaleFileContent = async (filePath) => {
      const fileContent = await fs_1.promises.readFile((0, path_1.join)(...getI18nBundleFolderPath(), filePath));
      return JSON.parse(fileContent.toString());
    };
    var makeResourceAccessorErrorMessage = (message) => {
      if (global.__forge_tunnel__) {
        const cliUpdateWarning = "To access i18n resources while using `forge tunnel`, please ensure that your Forge CLI is up to date. Run `npm install -g @forge/cli` to update to the latest version.";
        return `${message}
${cliUpdateWarning}`;
      }
      return message;
    };
    var resolverResourcesAccessor = {
      getI18nInfoConfig: async () => {
        try {
          const info = await readLocaleFileContent(i18n_1.I18N_INFO_FILE_NAME);
          return info.config;
        } catch (error) {
          throw new i18n_1.TranslationGetterError(makeResourceAccessorErrorMessage("Failed to get i18n info config."));
        }
      },
      getTranslationResource: async (locale) => {
        try {
          return await readLocaleFileContent(`${locale}.json`);
        } catch (error) {
          throw new i18n_1.TranslationGetterError(makeResourceAccessorErrorMessage(`Failed to get translation resource for locale: ${locale}.`));
        }
      }
    };
    var translationsFunctionCache = /* @__PURE__ */ new Map();
    var translationsGetter = new i18n_1.TranslationsGetter(resolverResourcesAccessor);
    var resetTranslationsCache = () => {
      translationsGetter.reset();
      translationsFunctionCache.clear();
    };
    exports2.resetTranslationsCache = resetTranslationsCache;
    var getTranslations = async (rawLocale, options = {
      fallback: true
    }) => {
      const locale = doEnsureLocale(rawLocale);
      return await translationsGetter.getTranslations(locale, options);
    };
    exports2.getTranslations = getTranslations;
    var createTranslationFunction = async (rawLocale) => {
      const locale = doEnsureLocale(rawLocale);
      let translator = translationsFunctionCache.get(locale);
      if (!translator) {
        translator = await createTranslationFunctionImpl(locale);
        translationsFunctionCache.set(locale, translator);
      }
      return translator;
    };
    exports2.createTranslationFunction = createTranslationFunction;
    var doEnsureLocale = (rawLocale) => {
      const ensuredLocale = (0, i18n_1.ensureLocale)(rawLocale);
      if (!ensuredLocale) {
        console.warn(`The locale "${rawLocale}" is not supported, defaulting to the default locale.`);
        return rawLocale;
      }
      return ensuredLocale;
    };
    var createTranslationFunctionImpl = async (locale) => {
      const translator = new i18n_1.Translator(locale, translationsGetter);
      await translator.init();
      return (i18nKey, defaultValue) => translator.translate(i18nKey) ?? defaultValue ?? i18nKey;
    };
    exports2.i18n = {
      createTranslationFunction: exports2.createTranslationFunction,
      getTranslations: exports2.getTranslations
    };
  }
});

// ../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/@forge/egress/out/egress/url-parser.js
var require_url_parser = __commonJS({
  "../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/@forge/egress/out/egress/url-parser.js"(exports2) {
    "use strict";
    Object.defineProperty(exports2, "__esModule", { value: true });
    exports2.parseUrl = parseUrl;
    function parseUrl(url) {
      var _a, _b;
      const protocol = (_b = (_a = url.match(/^(.*?:)/)) === null || _a === void 0 ? void 0 : _a[0]) !== null && _b !== void 0 ? _b : "https:";
      const hostAndPath = url.replace(protocol, "").replace(/^\/*/, "").replace(/^\\*/, "").split("?")[0].split("#")[0];
      const hostname = hostAndPath.split("/")[0];
      const pathname = hostAndPath.slice(hostname.length) || "/";
      return { protocol, hostname, pathname };
    }
  }
});

// ../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/@forge/egress/out/egress/utils.js
var require_utils = __commonJS({
  "../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/@forge/egress/out/egress/utils.js"(exports2) {
    "use strict";
    Object.defineProperty(exports2, "__esModule", { value: true });
    exports2.getEgressesBasedOnToggles = exports2.sortAndGroupEgressPermissionsByDomain = exports2.EgressCategory = exports2.EgressType = void 0;
    exports2.globToRegex = globToRegex;
    var url_parser_1 = require_url_parser();
    function globToRegex(pattern) {
      const escaped = pattern.replace(/[.+?^${}()|[\]\\]/g, "\\$&");
      const regexPattern = escaped.replace(/\*/g, ".*");
      return new RegExp(`^${regexPattern}$`);
    }
    var sortAndGroupEgressPermissionsByDomain = (egressAddresses) => {
      if ((egressAddresses === null || egressAddresses === void 0 ? void 0 : egressAddresses.length) === 0) {
        return [];
      }
      const protocolRegex = /^(.*?:\/\/)/;
      const domains = /* @__PURE__ */ new Set();
      const wildcardDomains = [];
      egressAddresses.forEach((item) => {
        const itemWithProtocol = protocolRegex.test(item) ? item : `https://${item}`;
        const url = (0, url_parser_1.parseUrl)(itemWithProtocol);
        if (url.hostname.startsWith("*")) {
          domains.add(url.hostname.substring(2));
          wildcardDomains.push(globToRegex(url.hostname));
        } else {
          domains.add(url.hostname);
        }
      });
      return [...domains].sort().reduce((grouped, domain) => {
        if (!wildcardDomains.some((pattern) => pattern.test(domain))) {
          grouped.push(domain);
        }
        return grouped;
      }, []);
    };
    exports2.sortAndGroupEgressPermissionsByDomain = sortAndGroupEgressPermissionsByDomain;
    var EgressType;
    (function(EgressType2) {
      EgressType2["FetchBackendSide"] = "FETCH_BACKEND_SIDE";
      EgressType2["FetchClientSide"] = "FETCH_CLIENT_SIDE";
      EgressType2["Fonts"] = "FONTS";
      EgressType2["Frames"] = "FRAMES";
      EgressType2["Images"] = "IMAGES";
      EgressType2["Media"] = "MEDIA";
      EgressType2["Scripts"] = "SCRIPTS";
      EgressType2["Styles"] = "STYLES";
    })(EgressType || (exports2.EgressType = EgressType = {}));
    var EgressCategory;
    (function(EgressCategory2) {
      EgressCategory2["ANALYTICS"] = "ANALYTICS";
    })(EgressCategory || (exports2.EgressCategory = EgressCategory = {}));
    var getEgressesBasedOnToggles = (input) => {
      const filteredEgresses = input.egress.filter((egress) => {
        var _a;
        if (((_a = egress.category) === null || _a === void 0 ? void 0 : _a.toUpperCase()) === EgressCategory.ANALYTICS) {
          if (input.installationConfig) {
            const analyticsConfig = input.installationConfig.find((config) => config.key.toUpperCase() === "ALLOW_EGRESS_ANALYTICS");
            return (analyticsConfig === null || analyticsConfig === void 0 ? void 0 : analyticsConfig.value) !== false;
          } else {
            return input.overrides.ALLOW_EGRESS_ANALYTICS !== false;
          }
        }
        return true;
      });
      const egressByType = /* @__PURE__ */ new Map();
      for (const egress of filteredEgresses) {
        if (!egressByType.has(egress.type)) {
          egressByType.set(egress.type, egress.addresses);
        }
        egressByType.set(egress.type, [...egressByType.get(egress.type), ...egress.addresses]);
      }
      return [...egressByType.entries()].map(([type, egresses]) => ({
        type,
        addresses: [...new Set(egresses)]
      }));
    };
    exports2.getEgressesBasedOnToggles = getEgressesBasedOnToggles;
  }
});

// ../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/@forge/egress/out/egress/egress-filtering-service.js
var require_egress_filtering_service = __commonJS({
  "../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/@forge/egress/out/egress/egress-filtering-service.js"(exports2) {
    "use strict";
    Object.defineProperty(exports2, "__esModule", { value: true });
    exports2.EgressFilteringService = void 0;
    var url_parser_1 = require_url_parser();
    var utils_1 = require_utils();
    var EgressFilteringService = class {
      constructor(allowList) {
        this.URLs = allowList.filter((domainOrURL) => !domainOrURL.startsWith("*")).map((url) => this.parseUrl(url));
        this.wildcardDomains = allowList.filter((domainOrURL) => domainOrURL !== "*").map((url) => this.parseUrl(url)).filter((url) => decodeURIComponent(url.hostname).startsWith("*")).map((url) => ({
          ...url,
          regex: (0, utils_1.globToRegex)(decodeURIComponent(url.hostname))
        }));
        this.allowsEverything = allowList.includes("*");
      }
      parseUrl(url) {
        return (0, url_parser_1.parseUrl)(url);
      }
      containsWildCardEgress() {
        return this.allowsEverything;
      }
      isValidUrl(url) {
        if (this.allowsEverything) {
          return true;
        }
        const parsedUrl = this.parseUrl(url);
        return this.allowedDomainExact(parsedUrl, this.URLs) || this.allowedDomainPattern(parsedUrl, this.wildcardDomains);
      }
      isValidUrlCSP(url) {
        if (this.allowsEverything) {
          return true;
        }
        const parsedUrl = this.parseUrl(url);
        return this.allowedDomainExactAndPath(parsedUrl, this.URLs) || this.allowedDomainPatternAndPath(parsedUrl, this.wildcardDomains);
      }
      allowedDomainExact(domain, allowList) {
        return allowList.filter((allowed) => allowed.protocol === domain.protocol).some((url) => url.hostname === domain.hostname);
      }
      allowedDomainExactAndPath(domain, allowList) {
        return allowList.filter((allowed) => this.protocolMatchesCSP(allowed.protocol, domain.protocol)).filter((allowed) => allowed.hostname === domain.hostname).some((allowed) => this.pathMatches(allowed.pathname, domain.pathname));
      }
      allowedDomainPattern(domain, allowList) {
        return allowList.filter((allowed) => allowed.protocol === domain.protocol).some((pattern) => pattern.regex.test(domain.hostname));
      }
      allowedDomainPatternAndPath(domain, allowList) {
        return allowList.filter((pattern) => this.protocolMatchesCSP(pattern.protocol, domain.protocol)).filter((pattern) => pattern.regex.test(domain.hostname)).some((allowed) => this.pathMatches(allowed.pathname, domain.pathname));
      }
      protocolMatchesCSP(allowedProtocol, requestProtocol) {
        if (allowedProtocol === requestProtocol) {
          return true;
        }
        if (allowedProtocol === "http:" && requestProtocol === "https:") {
          return true;
        }
        if (allowedProtocol === "ws:" && requestProtocol === "wss:") {
          return true;
        }
        return false;
      }
      pathMatches(allowedPath, requestPath) {
        if (allowedPath === "/") {
          return true;
        }
        if (allowedPath.endsWith("/")) {
          return requestPath.startsWith(allowedPath);
        }
        return requestPath === allowedPath;
      }
    };
    exports2.EgressFilteringService = EgressFilteringService;
  }
});

// ../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/@forge/egress/out/egress/index.js
var require_egress = __commonJS({
  "../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/@forge/egress/out/egress/index.js"(exports2) {
    "use strict";
    Object.defineProperty(exports2, "__esModule", { value: true });
    var tslib_1 = (init_tslib_es6(), __toCommonJS(tslib_es6_exports));
    tslib_1.__exportStar(require_egress_filtering_service(), exports2);
    tslib_1.__exportStar(require_url_parser(), exports2);
    tslib_1.__exportStar(require_utils(), exports2);
  }
});

// ../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/@forge/egress/out/index.js
var require_out3 = __commonJS({
  "../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/@forge/egress/out/index.js"(exports2) {
    "use strict";
    Object.defineProperty(exports2, "__esModule", { value: true });
    var tslib_1 = (init_tslib_es6(), __toCommonJS(tslib_es6_exports));
    tslib_1.__exportStar(require_egress(), exports2);
  }
});

// ../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/@forge/api/out/api/permissions.js
var require_permissions3 = __commonJS({
  "../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/@forge/api/out/api/permissions.js"(exports2) {
    "use strict";
    Object.defineProperty(exports2, "__esModule", { value: true });
    exports2.permissions = exports2.canLoadResource = exports2.canFetchFrom = exports2.hasScope = exports2.hasPermission = void 0;
    exports2.extractUrlString = extractUrlString;
    var runtime_1 = require_runtime();
    var errors_1 = require_errors();
    var egress_1 = require_out3();
    function extractUrlString(url) {
      if (typeof url === "string") {
        return url;
      }
      if ("address" in url) {
        return url.address;
      }
      return url.remote;
    }
    function wrapInSyncMetrics(options, cb) {
      const metrics = (0, runtime_1.__getRuntime)().metrics;
      metrics.counter(options.name, options.tags).incr();
      return cb();
    }
    var getMissingScopes = (requiredScopes, currentlyGrantedScopes) => {
      if (!requiredScopes) {
        return void 0;
      }
      if (Array.isArray(requiredScopes) && requiredScopes.length > 0) {
        const currentGrantedScopes = Array.isArray(currentlyGrantedScopes) ? currentlyGrantedScopes : [];
        const missingScopes = requiredScopes.filter((scope) => !currentGrantedScopes.includes(scope));
        if (missingScopes.length > 0) {
          return missingScopes;
        }
      }
      return void 0;
    };
    var getMissingUrls = (requiredUrls, currentlyGrantedUrls, useCSP) => {
      const allowList = currentlyGrantedUrls.map((url) => extractUrlString(url));
      const egressFilter = new egress_1.EgressFilteringService(allowList);
      const missingUrls = requiredUrls.filter((requiredUrl) => {
        const urlString = extractUrlString(requiredUrl);
        if (useCSP) {
          return !egressFilter.isValidUrlCSP(urlString);
        }
        return !egressFilter.isValidUrl(urlString);
      });
      return missingUrls;
    };
    var VALID_REQUIREMENT_KEYS = ["scopes", "external"];
    var VALID_EXTERNAL_TYPES = [
      "fetch",
      "fonts",
      "frames",
      "images",
      "media",
      "scripts",
      "styles",
      "configurable"
    ];
    var VALID_FETCH_TYPES = ["backend", "client"];
    var validateKeys = (obj, validKeys) => {
      const validKeysSet = new Set(validKeys);
      const providedKeys = Object.keys(obj);
      const invalidKeys = providedKeys.filter((key) => !validKeysSet.has(key));
      if (invalidKeys.length > 0) {
        throw new Error(`Invalid permission key(s): ${invalidKeys.join(", ")}. Visit https://go.atlassian.com/forge-permissions for more information.`);
      }
    };
    var validateObjectField = (value, fieldPath) => {
      if (value !== void 0) {
        if (typeof value !== "object" || value === null || Array.isArray(value)) {
          throw new TypeError(`${fieldPath} should be an object, not ${Array.isArray(value) ? "an array" : `a ${typeof value}`}`);
        }
      }
    };
    var validateArrayField = (value, fieldPath) => {
      if (value !== void 0 && !Array.isArray(value)) {
        throw new TypeError(`${fieldPath} should be an array, not a ${typeof value}`);
      }
    };
    var validatePermissionRequirements = (requirements) => {
      validateKeys(requirements, VALID_REQUIREMENT_KEYS);
      validateArrayField(requirements.scopes, "scopes");
      if (requirements.external !== void 0) {
        validateObjectField(requirements.external, "external");
        validateKeys(requirements.external, VALID_EXTERNAL_TYPES);
        for (const type of VALID_EXTERNAL_TYPES) {
          if (type !== "fetch" && type !== "configurable") {
            validateArrayField(requirements.external[type], `external.${String(type)}`);
          }
        }
        if (requirements.external.fetch !== void 0) {
          validateObjectField(requirements.external.fetch, "external.fetch");
          validateKeys(requirements.external.fetch, VALID_FETCH_TYPES);
          for (const type of VALID_FETCH_TYPES) {
            validateArrayField(requirements.external.fetch[type], `external.fetch.${String(type)}`);
          }
        }
      }
    };
    var getMissingFetchPermissions = (requiredFetch, currentlyGrantedFetch) => {
      if (!requiredFetch) {
        return void 0;
      }
      const missingFetch = {};
      Object.keys(requiredFetch).forEach((fetchType) => {
        const requiredUrls = requiredFetch[fetchType];
        if (!requiredUrls || !Array.isArray(requiredUrls) || requiredUrls.length === 0)
          return;
        const missingUrls = getMissingUrls(requiredUrls, currentlyGrantedFetch?.[fetchType] ?? [], fetchType === "client");
        if (missingUrls.length) {
          missingFetch[fetchType] = missingUrls.map(extractUrlString);
        }
      });
      return Object.keys(missingFetch).length ? missingFetch : void 0;
    };
    var getMissingExternalPermissions = (requiredExternal, currentGrantedExternal) => {
      let missingExternal = void 0;
      Object.keys(requiredExternal).forEach((type) => {
        if (type === "fetch") {
          const missingFetchPerms = getMissingFetchPermissions(requiredExternal.fetch, currentGrantedExternal.fetch);
          if (missingFetchPerms) {
            if (!missingExternal) {
              missingExternal = {};
            }
            missingExternal.fetch = missingFetchPerms;
          }
          return;
        }
        if (type === "configurable") {
          return;
        }
        const externalUrls = requiredExternal[type];
        if (!externalUrls || !Array.isArray(externalUrls) || externalUrls.length === 0) {
          return;
        }
        const missingUrls = getMissingUrls(externalUrls, currentGrantedExternal[type] || [], true);
        if (missingUrls.length > 0) {
          if (!missingExternal) {
            missingExternal = {};
          }
          missingExternal[type] = missingUrls.map(extractUrlString);
        }
      });
      return missingExternal;
    };
    var hasPermission = (requirements) => {
      return wrapInSyncMetrics({ name: "api.permissions.hasPermission" }, () => hasPermissionWithoutMetrics(requirements));
    };
    exports2.hasPermission = hasPermission;
    var hasPermissionWithoutMetrics = (requirements) => {
      const appContext = (0, runtime_1.getAppContext)();
      const currentlyGrantedPermissions = appContext.permissions;
      const arePermissionsAvailable = !!(currentlyGrantedPermissions && typeof currentlyGrantedPermissions === "object");
      if (!arePermissionsAvailable) {
        throw new errors_1.ApiNotReadyError("This feature is not available yet");
      }
      validatePermissionRequirements(requirements);
      const missingPermissions = {};
      let hasMissingPermissions = false;
      const missingScopes = getMissingScopes(requirements.scopes, currentlyGrantedPermissions.scopes);
      if (missingScopes) {
        missingPermissions.scopes = missingScopes;
        hasMissingPermissions = true;
      }
      if (requirements.external) {
        const { external: requiredExternal } = requirements;
        const currentlyGrantedExternal = currentlyGrantedPermissions.external || {};
        const missingExternalPerms = getMissingExternalPermissions(requiredExternal, currentlyGrantedExternal);
        if (missingExternalPerms) {
          missingPermissions.external = {
            ...missingPermissions.external,
            ...missingExternalPerms
          };
          hasMissingPermissions = true;
        }
      }
      return {
        granted: !hasMissingPermissions,
        ...hasMissingPermissions && {
          missing: new runtime_1.MissingPermissions(missingPermissions.scopes, missingPermissions.external)
        }
      };
    };
    var hasScope = (scope) => {
      return wrapInSyncMetrics({ name: "api.permissions.hasScope" }, () => hasPermissionWithoutMetrics({ scopes: [scope] }).granted);
    };
    exports2.hasScope = hasScope;
    var canFetchFrom = (type, url) => {
      return wrapInSyncMetrics({ name: "api.permissions.canFetchFrom" }, () => hasPermissionWithoutMetrics({ external: { fetch: { [type]: [url] } } }).granted);
    };
    exports2.canFetchFrom = canFetchFrom;
    var canLoadResource = (type, url) => {
      return wrapInSyncMetrics({ name: "api.permissions.canLoadResource" }, () => hasPermissionWithoutMetrics({ external: { [type]: [url] } }).granted);
    };
    exports2.canLoadResource = canLoadResource;
    exports2.permissions = {
      hasPermission: exports2.hasPermission,
      hasScope: exports2.hasScope,
      canFetchFrom: exports2.canFetchFrom,
      canLoadResource: exports2.canLoadResource
    };
  }
});

// ../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/@forge/api/out/index.js
var require_out4 = __commonJS({
  "../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/@forge/api/out/index.js"(exports2) {
    "use strict";
    Object.defineProperty(exports2, "__esModule", { value: true });
    exports2.permissions = exports2.i18n = exports2.routeFromAbsolute = exports2.route = exports2.assumeTrustedRoute = exports2.getAppContext = exports2.bindInvocationContext = exports2.__getRuntime = exports2.RequestProductNotAllowedError = exports2.ProxyRequestError = exports2.ProductEndpointNotAllowedError = exports2.NotAllowedError = exports2.NeedsAuthenticationError = exports2.isHostedCodeError = exports2.isForgePlatformError = exports2.isExpectedError = exports2.InvalidWorkspaceRequestedError = exports2.HttpError = exports2.FUNCTION_ERR = exports2.FetchError = exports2.ExternalEndpointNotAllowedError = exports2.createRequestStargateAsApp = exports2.webTrigger = exports2.requestJira = exports2.requestConfluence = exports2.requestBitbucket = exports2.invokeService = exports2.invokeRemote = exports2.fetch = exports2.authorize = exports2.asUser = exports2.asApp = exports2.__requestAtlassianAsUser = exports2.__requestAtlassianAsApp = exports2.__fetchProduct = exports2.privacy = void 0;
    var endpoint_1 = require_endpoint();
    Object.defineProperty(exports2, "invokeRemote", { enumerable: true, get: function() {
      return endpoint_1.invokeRemote;
    } });
    Object.defineProperty(exports2, "invokeService", { enumerable: true, get: function() {
      return endpoint_1.invokeService;
    } });
    var fetch_1 = require_fetch();
    Object.defineProperty(exports2, "__fetchProduct", { enumerable: true, get: function() {
      return fetch_1.__fetchProduct;
    } });
    Object.defineProperty(exports2, "__requestAtlassianAsApp", { enumerable: true, get: function() {
      return fetch_1.__requestAtlassianAsApp;
    } });
    Object.defineProperty(exports2, "__requestAtlassianAsUser", { enumerable: true, get: function() {
      return fetch_1.__requestAtlassianAsUser;
    } });
    var authorization_1 = require_authorization();
    Object.defineProperty(exports2, "authorize", { enumerable: true, get: function() {
      return authorization_1.authorize;
    } });
    var privacy_1 = require_privacy();
    var webTrigger_1 = require_webTrigger();
    Object.defineProperty(exports2, "webTrigger", { enumerable: true, get: function() {
      return webTrigger_1.webTrigger;
    } });
    var fetchAPI = (0, fetch_1.getFetchAPI)();
    var asUser = fetchAPI.asUser;
    exports2.asUser = asUser;
    var asApp = fetchAPI.asApp;
    exports2.asApp = asApp;
    var fetch2 = fetchAPI.fetch;
    exports2.fetch = fetch2;
    var requestJira = fetchAPI.requestJira;
    exports2.requestJira = requestJira;
    var requestConfluence = fetchAPI.requestConfluence;
    exports2.requestConfluence = requestConfluence;
    var requestBitbucket = fetchAPI.requestBitbucket;
    exports2.requestBitbucket = requestBitbucket;
    var API = {
      ...fetchAPI,
      invokeRemote: endpoint_1.invokeRemote,
      invokeService: endpoint_1.invokeService
    };
    exports2.privacy = {
      reportPersonalData: (0, privacy_1.createReportPersonalData)(fetch_1.__requestAtlassianAsApp)
    };
    exports2.default = API;
    var createRequestStargateAsApp = () => fetch_1.__requestAtlassianAsApp;
    exports2.createRequestStargateAsApp = createRequestStargateAsApp;
    var errors_1 = require_errors();
    Object.defineProperty(exports2, "ExternalEndpointNotAllowedError", { enumerable: true, get: function() {
      return errors_1.ExternalEndpointNotAllowedError;
    } });
    Object.defineProperty(exports2, "FetchError", { enumerable: true, get: function() {
      return errors_1.FetchError;
    } });
    Object.defineProperty(exports2, "FUNCTION_ERR", { enumerable: true, get: function() {
      return errors_1.FUNCTION_ERR;
    } });
    Object.defineProperty(exports2, "HttpError", { enumerable: true, get: function() {
      return errors_1.HttpError;
    } });
    Object.defineProperty(exports2, "InvalidWorkspaceRequestedError", { enumerable: true, get: function() {
      return errors_1.InvalidWorkspaceRequestedError;
    } });
    Object.defineProperty(exports2, "isExpectedError", { enumerable: true, get: function() {
      return errors_1.isExpectedError;
    } });
    Object.defineProperty(exports2, "isForgePlatformError", { enumerable: true, get: function() {
      return errors_1.isForgePlatformError;
    } });
    Object.defineProperty(exports2, "isHostedCodeError", { enumerable: true, get: function() {
      return errors_1.isHostedCodeError;
    } });
    Object.defineProperty(exports2, "NeedsAuthenticationError", { enumerable: true, get: function() {
      return errors_1.NeedsAuthenticationError;
    } });
    Object.defineProperty(exports2, "NotAllowedError", { enumerable: true, get: function() {
      return errors_1.NotAllowedError;
    } });
    Object.defineProperty(exports2, "ProductEndpointNotAllowedError", { enumerable: true, get: function() {
      return errors_1.ProductEndpointNotAllowedError;
    } });
    Object.defineProperty(exports2, "ProxyRequestError", { enumerable: true, get: function() {
      return errors_1.ProxyRequestError;
    } });
    Object.defineProperty(exports2, "RequestProductNotAllowedError", { enumerable: true, get: function() {
      return errors_1.RequestProductNotAllowedError;
    } });
    var runtime_1 = require_runtime();
    Object.defineProperty(exports2, "__getRuntime", { enumerable: true, get: function() {
      return runtime_1.__getRuntime;
    } });
    Object.defineProperty(exports2, "bindInvocationContext", { enumerable: true, get: function() {
      return runtime_1.bindInvocationContext;
    } });
    Object.defineProperty(exports2, "getAppContext", { enumerable: true, get: function() {
      return runtime_1.getAppContext;
    } });
    var safeUrl_1 = require_safeUrl();
    Object.defineProperty(exports2, "assumeTrustedRoute", { enumerable: true, get: function() {
      return safeUrl_1.assumeTrustedRoute;
    } });
    Object.defineProperty(exports2, "route", { enumerable: true, get: function() {
      return safeUrl_1.route;
    } });
    Object.defineProperty(exports2, "routeFromAbsolute", { enumerable: true, get: function() {
      return safeUrl_1.routeFromAbsolute;
    } });
    var i18n_1 = require_i18n();
    Object.defineProperty(exports2, "i18n", { enumerable: true, get: function() {
      return i18n_1.i18n;
    } });
    var permissions_1 = require_permissions3();
    Object.defineProperty(exports2, "permissions", { enumerable: true, get: function() {
      return permissions_1.permissions;
    } });
  }
});

// ../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/@forge/events/out/queries.js
var require_queries = __commonJS({
  "../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/@forge/events/out/queries.js"(exports2) {
    "use strict";
    Object.defineProperty(exports2, "__esModule", { value: true });
    exports2.post = exports2.CANCEL_JOB_PATH = exports2.GET_STATS_PATH = exports2.PUSH_PATH = void 0;
    exports2.PUSH_PATH = "/webhook/queue/publish/{contextAri}/{environmentId}/{appId}/{appVersion}";
    exports2.GET_STATS_PATH = "/webhook/queue/stats/{contextAri}/{environmentId}/{appId}/{appVersion}";
    exports2.CANCEL_JOB_PATH = "/webhook/queue/cancel/{contextAri}/{environmentId}/{appId}/{appVersion}";
    var post = async (endpoint, body, apiClient) => {
      const request = {
        method: "POST",
        body: JSON.stringify(body),
        headers: {
          "content-type": "application/json"
        }
      };
      return await apiClient(endpoint, request);
    };
    exports2.post = post;
  }
});

// ../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/@forge/events/out/errors.js
var require_errors2 = __commonJS({
  "../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/@forge/events/out/errors.js"(exports2) {
    "use strict";
    Object.defineProperty(exports2, "__esModule", { value: true });
    exports2.InvocationLimitReachedError = exports2.JobDoesNotExistError = exports2.InternalServerError = exports2.PartialSuccessError = exports2.RateLimitError = exports2.NoEventsToPushError = exports2.PayloadTooBigError = exports2.TooManyEventsError = exports2.InvalidPayloadError = exports2.InvalidQueueNameError = exports2.InvalidPushSettingsError = exports2.EventsError = void 0;
    var EventsError = class extends Error {
      constructor(message) {
        super(message);
      }
    };
    exports2.EventsError = EventsError;
    var InvalidPushSettingsError = class extends EventsError {
    };
    exports2.InvalidPushSettingsError = InvalidPushSettingsError;
    var InvalidQueueNameError = class extends EventsError {
    };
    exports2.InvalidQueueNameError = InvalidQueueNameError;
    var InvalidPayloadError = class extends EventsError {
    };
    exports2.InvalidPayloadError = InvalidPayloadError;
    var TooManyEventsError = class extends EventsError {
    };
    exports2.TooManyEventsError = TooManyEventsError;
    var PayloadTooBigError = class extends EventsError {
    };
    exports2.PayloadTooBigError = PayloadTooBigError;
    var NoEventsToPushError = class extends EventsError {
    };
    exports2.NoEventsToPushError = NoEventsToPushError;
    var RateLimitError = class extends EventsError {
    };
    exports2.RateLimitError = RateLimitError;
    var PartialSuccessError = class extends EventsError {
      result;
      failedEvents;
      constructor(message, result, failedEvents) {
        super(message);
        this.result = result;
        this.failedEvents = failedEvents;
      }
    };
    exports2.PartialSuccessError = PartialSuccessError;
    var InternalServerError = class extends EventsError {
      constructor(message, errorCode, details) {
        super(message);
        this.errorCode = errorCode;
        this.details = details;
      }
      errorCode;
      details;
    };
    exports2.InternalServerError = InternalServerError;
    var JobDoesNotExistError = class extends EventsError {
    };
    exports2.JobDoesNotExistError = JobDoesNotExistError;
    var InvocationLimitReachedError = class extends EventsError {
    };
    exports2.InvocationLimitReachedError = InvocationLimitReachedError;
  }
});

// ../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/@forge/events/out/text.js
var require_text = __commonJS({
  "../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/@forge/events/out/text.js"(exports2) {
    "use strict";
    Object.defineProperty(exports2, "__esModule", { value: true });
    exports2.Text = void 0;
    exports2.Text = {
      error: {
        invalidQueueName: `Queue names can only contain alphanumeric characters, dashes and underscores.`,
        invalidEvent: `Event must be an object.`,
        invalidEventBody: `Event body must be an object.`,
        invalidDelayInSecondsSetting: `The delayInSeconds setting must be between 0 and 900.`,
        maxEventsAllowed: (maxEventsCount) => `This push contains more than the ${maxEventsCount} events allowed.`,
        maxPayloadAllowed: (maxPayloadSize) => `The maximum payload size is ${maxPayloadSize}KB.`,
        noEventsPushed: `No events pushed.`,
        rateLimitError: `Too many requests.`,
        invocationLimitReachedError: `The limit on cyclic invocation has been reached.`,
        jobIdEmpty: `jobId cannot be empty.`,
        jobDoesNotExit: (jobId, queueName) => `The job ${jobId} was not found for the queue ${queueName}.`
      }
    };
  }
});

// ../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/@forge/events/out/validators.js
var require_validators = __commonJS({
  "../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/@forge/events/out/validators.js"(exports2) {
    "use strict";
    Object.defineProperty(exports2, "__esModule", { value: true });
    exports2.validateCancelJobAPIResponse = exports2.validateGetStatsAPIResponse = exports2.validatePushAPIResponse = exports2.validateAPIResponse = exports2.validateCancelJobRequest = exports2.validateGetStatsPayload = exports2.validatePushSettings = exports2.validateQueueKey = void 0;
    exports2.validatePushEvents = validatePushEvents;
    var errors_1 = require_errors2();
    var text_1 = require_text();
    var VALID_QUEUE_NAME_PATTERN = /^[a-zA-Z0-9-_]+$/;
    var MAXIMUM_EVENTS = 50;
    var MAXIMUM_PAYLOAD_SIZE_KB = 200;
    var validateQueueKey = (queueName) => {
      if (!queueName || !VALID_QUEUE_NAME_PATTERN.test(queueName)) {
        throw new errors_1.InvalidQueueNameError(text_1.Text.error.invalidQueueName);
      }
    };
    exports2.validateQueueKey = validateQueueKey;
    var validatePushSettings = (event) => {
      if (event.delayInSeconds && event.delayInSeconds > 900 || event.delayInSeconds && event.delayInSeconds < 0) {
        throw new errors_1.InvalidPushSettingsError(text_1.Text.error.invalidDelayInSecondsSetting);
      }
    };
    exports2.validatePushSettings = validatePushSettings;
    function validatePushEvents(arg) {
      const events = Array.isArray(arg) ? arg : [arg];
      if (events.length === 0) {
        throw new errors_1.NoEventsToPushError(text_1.Text.error.noEventsPushed);
      }
      if (events.length > MAXIMUM_EVENTS) {
        throw new errors_1.TooManyEventsError(text_1.Text.error.maxEventsAllowed(MAXIMUM_EVENTS));
      }
      for (const event of events) {
        if (typeof event !== "object" || Array.isArray(event) || event === null) {
          throw new errors_1.InvalidPayloadError(text_1.Text.error.invalidEvent);
        }
        if (typeof event.body !== "object" || Array.isArray(event.body) || event.body === null) {
          throw new errors_1.InvalidPayloadError(text_1.Text.error.invalidEventBody);
        }
        (0, exports2.validatePushSettings)(event);
      }
      const payloadSizeKB = Buffer.byteLength(JSON.stringify(events)) / 1024;
      if (payloadSizeKB > MAXIMUM_PAYLOAD_SIZE_KB) {
        throw new errors_1.PayloadTooBigError(text_1.Text.error.maxPayloadAllowed(MAXIMUM_PAYLOAD_SIZE_KB));
      }
      return events;
    }
    var validateGetStatsPayload = (getStatsRequest) => {
      if (!getStatsRequest.jobId) {
        throw new errors_1.JobDoesNotExistError(text_1.Text.error.jobIdEmpty);
      }
      (0, exports2.validateQueueKey)(getStatsRequest.queueName);
    };
    exports2.validateGetStatsPayload = validateGetStatsPayload;
    var validateCancelJobRequest = (cancelJobRequest) => {
      if (!cancelJobRequest.jobId) {
        throw new errors_1.JobDoesNotExistError(text_1.Text.error.jobIdEmpty);
      }
      (0, exports2.validateQueueKey)(cancelJobRequest.queueName);
    };
    exports2.validateCancelJobRequest = validateCancelJobRequest;
    var validateAPIResponse = async (response, expectedSuccessStatus) => {
      if (response.status === 429) {
        throw new errors_1.RateLimitError(text_1.Text.error.rateLimitError);
      }
      if (response.status === 405) {
        throw new errors_1.InvocationLimitReachedError(text_1.Text.error.invocationLimitReachedError);
      }
      if (response.status != expectedSuccessStatus && response.status) {
        let internalServerError;
        try {
          const responseBody = await response.json();
          const errorMessage = responseBody.message ? `: ${responseBody.message}` : "";
          const errors = responseBody.errors ? `: ${responseBody.errors.join(", ")}` : "";
          internalServerError = new errors_1.InternalServerError(`${response.status} ${response.statusText}${errorMessage}${errors}`, responseBody.code, responseBody.details);
        } catch (ignore) {
          internalServerError = new errors_1.InternalServerError(`${response.status} ${response.statusText}`, response.status);
        }
        throw internalServerError;
      }
    };
    exports2.validateAPIResponse = validateAPIResponse;
    var validatePushAPIResponse = async (requestBody, response, result) => {
      if (response.status === 413) {
        const responseBody = await response.json();
        throw new errors_1.PayloadTooBigError(responseBody.errorMessage);
      }
      if (response.status === 202) {
        const responseBody = await response.json();
        const defaultErrorMessage = "Failed to process some events.";
        const partialSuccessError = new errors_1.PartialSuccessError(defaultErrorMessage, result, []);
        if (responseBody.failedEvents && responseBody.failedEvents.length > 0) {
          partialSuccessError.message = `Failed to process ${responseBody.failedEvents.length} event(s).`;
          partialSuccessError.failedEvents = responseBody.failedEvents.map((failedEvent) => {
            return {
              errorMessage: failedEvent.errorMessage,
              payload: requestBody.payload[+failedEvent.index]
            };
          });
        }
        if (responseBody.errorMessage) {
          partialSuccessError.message = partialSuccessError.message !== defaultErrorMessage ? `${partialSuccessError.message} ${responseBody.errorMessage}` : responseBody.errorMessage;
        }
        throw partialSuccessError;
      }
      await (0, exports2.validateAPIResponse)(response, 201);
    };
    exports2.validatePushAPIResponse = validatePushAPIResponse;
    var validateGetStatsAPIResponse = async (response, getStatsRequest) => {
      if (response.status === 404) {
        throw new errors_1.JobDoesNotExistError(text_1.Text.error.jobDoesNotExit(getStatsRequest.jobId, getStatsRequest.queueName));
      }
      await (0, exports2.validateAPIResponse)(response, 200);
    };
    exports2.validateGetStatsAPIResponse = validateGetStatsAPIResponse;
    var validateCancelJobAPIResponse = async (response, cancelJobRequest) => {
      if (response.status === 404) {
        throw new errors_1.JobDoesNotExistError(text_1.Text.error.jobDoesNotExit(cancelJobRequest.jobId, cancelJobRequest.queueName));
      }
      await (0, exports2.validateAPIResponse)(response, 204);
    };
    exports2.validateCancelJobAPIResponse = validateCancelJobAPIResponse;
  }
});

// ../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/@forge/events/out/jobProgress.js
var require_jobProgress = __commonJS({
  "../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/@forge/events/out/jobProgress.js"(exports2) {
    "use strict";
    Object.defineProperty(exports2, "__esModule", { value: true });
    exports2.JobProgress = void 0;
    var api_1 = require_out4();
    var queries_1 = require_queries();
    var validators_1 = require_validators();
    var JobProgress = class {
      queueParams;
      id;
      apiClient;
      constructor(queueParams, id, apiClient = api_1.__requestAtlassianAsApp) {
        this.queueParams = queueParams;
        this.id = id;
        this.apiClient = apiClient;
      }
      async getStats() {
        const getStatsRequest = {
          queueName: this.queueParams.key,
          jobId: this.id,
          time: (/* @__PURE__ */ new Date()).toISOString()
        };
        (0, validators_1.validateGetStatsPayload)(getStatsRequest);
        const response = await (0, queries_1.post)(queries_1.GET_STATS_PATH, getStatsRequest, this.apiClient);
        await (0, validators_1.validateGetStatsAPIResponse)(response, getStatsRequest);
        const { success, inProgress, failed } = await response.json();
        return { success, inProgress, failed };
      }
      async cancel() {
        const cancelJobRequest = {
          queueName: this.queueParams.key,
          jobId: this.id,
          time: (/* @__PURE__ */ new Date()).toISOString()
        };
        (0, validators_1.validateCancelJobRequest)(cancelJobRequest);
        const response = await (0, queries_1.post)(queries_1.CANCEL_JOB_PATH, cancelJobRequest, this.apiClient);
        await (0, validators_1.validateCancelJobAPIResponse)(response, cancelJobRequest);
      }
    };
    exports2.JobProgress = JobProgress;
  }
});

// ../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/@forge/events/out/queue.js
var require_queue = __commonJS({
  "../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/@forge/events/out/queue.js"(exports2) {
    "use strict";
    Object.defineProperty(exports2, "__esModule", { value: true });
    exports2.Queue = void 0;
    var api_1 = require_out4();
    var queries_1 = require_queries();
    var validators_1 = require_validators();
    var crypto_1 = require("crypto");
    var jobProgress_1 = require_jobProgress();
    var Queue2 = class {
      queueParams;
      apiClient;
      constructor(queueParams, apiClient = api_1.__requestAtlassianAsApp) {
        this.queueParams = queueParams;
        this.apiClient = apiClient;
        (0, validators_1.validateQueueKey)(this.queueParams.key);
      }
      async push(events) {
        const validEvents = (0, validators_1.validatePushEvents)(events);
        const queueName = this.queueParams.key;
        const jobId = (0, crypto_1.randomUUID)();
        const pushRequest = {
          queueName,
          jobId,
          type: "avi:forge:app:event",
          schema: "ari:cloud:ecosystem::forge/app-event-2",
          payload: validEvents,
          time: (/* @__PURE__ */ new Date()).toISOString()
        };
        const response = await (0, queries_1.post)(queries_1.PUSH_PATH, pushRequest, this.apiClient);
        const result = { jobId };
        await (0, validators_1.validatePushAPIResponse)(pushRequest, response, result);
        return result;
      }
      getJob(jobId) {
        return new jobProgress_1.JobProgress(this.queueParams, jobId, this.apiClient);
      }
      async cancel(jobId) {
        const job = this.getJob(jobId);
        await job.cancel();
      }
      async getStats(jobId) {
        const job = this.getJob(jobId);
        return job.getStats();
      }
    };
    exports2.Queue = Queue2;
  }
});

// ../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/@forge/events/out/queueResponse.js
var require_queueResponse = __commonJS({
  "../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/@forge/events/out/queueResponse.js"(exports2) {
    "use strict";
    Object.defineProperty(exports2, "__esModule", { value: true });
    exports2.QueueResponse = exports2.Response = void 0;
    var Response = class {
      _retry;
      constructor(_retry) {
        this._retry = _retry;
      }
    };
    exports2.Response = Response;
    var QueueResponse = class extends Response {
      constructor() {
        super(false);
      }
      retry() {
        this._retry = true;
      }
    };
    exports2.QueueResponse = QueueResponse;
  }
});

// ../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/@forge/events/out/invocationErrorCode.js
var require_invocationErrorCode = __commonJS({
  "../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/@forge/events/out/invocationErrorCode.js"(exports2) {
    "use strict";
    Object.defineProperty(exports2, "__esModule", { value: true });
    exports2.InvocationErrorCode = void 0;
    var InvocationErrorCode2;
    (function(InvocationErrorCode3) {
      InvocationErrorCode3["FUNCTION_OUT_OF_MEMORY"] = "FUNCTION_OUT_OF_MEMORY";
      InvocationErrorCode3["FUNCTION_TIME_OUT"] = "FUNCTION_TIME_OUT";
      InvocationErrorCode3["FUNCTION_PLATFORM_UNKNOWN_ERROR"] = "FUNCTION_PLATFORM_UNKNOWN_ERROR";
      InvocationErrorCode3["FUNCTION_PLATFORM_RATE_LIMITED"] = "FUNCTION_PLATFORM_RATE_LIMITED";
      InvocationErrorCode3["FUNCTION_UPSTREAM_RATE_LIMITED"] = "FUNCTION_UPSTREAM_RATE_LIMITED";
      InvocationErrorCode3["FUNCTION_RETRY_REQUEST"] = "FUNCTION_RETRY_REQUEST";
    })(InvocationErrorCode2 || (exports2.InvocationErrorCode = InvocationErrorCode2 = {}));
  }
});

// ../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/@forge/events/out/retryOptions.js
var require_retryOptions = __commonJS({
  "../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/@forge/events/out/retryOptions.js"(exports2) {
    "use strict";
    Object.defineProperty(exports2, "__esModule", { value: true });
    exports2.DEFAULT_RETRY_OPTIONS = exports2.MIN_RETRY_AFTER = void 0;
    var invocationErrorCode_1 = require_invocationErrorCode();
    exports2.MIN_RETRY_AFTER = 1;
    exports2.DEFAULT_RETRY_OPTIONS = {
      retryAfter: exports2.MIN_RETRY_AFTER,
      retryReason: invocationErrorCode_1.InvocationErrorCode.FUNCTION_RETRY_REQUEST
    };
  }
});

// ../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/@forge/events/out/invocationError.js
var require_invocationError = __commonJS({
  "../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/@forge/events/out/invocationError.js"(exports2) {
    "use strict";
    Object.defineProperty(exports2, "__esModule", { value: true });
    exports2.InvocationError = void 0;
    var retryOptions_1 = require_retryOptions();
    var queueResponse_1 = require_queueResponse();
    var InvocationError2 = class extends queueResponse_1.Response {
      retryOptions;
      constructor(retryOptions = retryOptions_1.DEFAULT_RETRY_OPTIONS) {
        super(true);
        this.retryOptions = retryOptions;
        if (this.retryOptions.retryAfter !== void 0 && this.retryOptions.retryAfter <= 0) {
          this.retryOptions.retryAfter = retryOptions_1.MIN_RETRY_AFTER;
        }
        return this.toJSON();
      }
      toJSON() {
        return {
          _retry: this._retry,
          retryOptions: this.retryOptions
        };
      }
    };
    exports2.InvocationError = InvocationError2;
  }
});

// ../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/@forge/events/out/appEvents.js
var require_appEvents = __commonJS({
  "../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/@forge/events/out/appEvents.js"(exports2) {
    "use strict";
    Object.defineProperty(exports2, "__esModule", { value: true });
    exports2.appEvents = void 0;
    var queries_1 = require_queries();
    var api_1 = require_out4();
    var errorTypes = {
      400: "VALIDATION_ERROR",
      401: "AUTHENTICATION_ERROR",
      403: "AUTHORIZATION_ERROR",
      429: "RATE_LIMIT",
      500: "SERVICE_ERROR",
      503: "SERVICE_UNAVAILABLE"
    };
    var endpoint = "/forge/events/v1/app-events";
    exports2.appEvents = {
      async publish(events) {
        const eventsArray = Array.isArray(events) ? events : [events];
        const body = {
          events: eventsArray.map((e) => ({
            key: e.key
          }))
        };
        const response = await (0, queries_1.post)(endpoint, body, api_1.__requestAtlassianAsApp);
        const responseBody = await response.json();
        if (!response.ok) {
          return {
            type: "error",
            errorType: errorTypes[response.status] ?? "OTHER",
            errorMessage: getErrorMessage(responseBody)
          };
        }
        return {
          type: "success",
          failedEvents: responseBody.failedEvents ?? []
        };
      }
    };
    function getErrorMessage(responseBody) {
      if (responseBody.errorMessages && responseBody.errorMessages.length > 0) {
        return responseBody.errorMessages.join(", ");
      } else if (responseBody.errors) {
        return Object.entries(responseBody.errors).map(([key, value]) => `${key}: ${value}`).join(", ");
      } else {
        return JSON.stringify(responseBody);
      }
    }
  }
});

// ../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/@forge/events/out/index.js
var require_out5 = __commonJS({
  "../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/@forge/events/out/index.js"(exports2) {
    "use strict";
    Object.defineProperty(exports2, "__esModule", { value: true });
    exports2.appEvents = exports2.InvocationErrorCode = exports2.InvocationError = exports2.QueueResponse = exports2.JobProgress = exports2.InvocationLimitReachedError = exports2.InvalidPushSettingsError = exports2.JobDoesNotExistError = exports2.InternalServerError = exports2.PartialSuccessError = exports2.RateLimitError = exports2.NoEventsToPushError = exports2.PayloadTooBigError = exports2.TooManyEventsError = exports2.InvalidQueueNameError = exports2.Queue = void 0;
    var queue_1 = require_queue();
    Object.defineProperty(exports2, "Queue", { enumerable: true, get: function() {
      return queue_1.Queue;
    } });
    var errors_1 = require_errors2();
    Object.defineProperty(exports2, "InvalidQueueNameError", { enumerable: true, get: function() {
      return errors_1.InvalidQueueNameError;
    } });
    Object.defineProperty(exports2, "TooManyEventsError", { enumerable: true, get: function() {
      return errors_1.TooManyEventsError;
    } });
    Object.defineProperty(exports2, "PayloadTooBigError", { enumerable: true, get: function() {
      return errors_1.PayloadTooBigError;
    } });
    Object.defineProperty(exports2, "NoEventsToPushError", { enumerable: true, get: function() {
      return errors_1.NoEventsToPushError;
    } });
    Object.defineProperty(exports2, "RateLimitError", { enumerable: true, get: function() {
      return errors_1.RateLimitError;
    } });
    Object.defineProperty(exports2, "PartialSuccessError", { enumerable: true, get: function() {
      return errors_1.PartialSuccessError;
    } });
    Object.defineProperty(exports2, "InternalServerError", { enumerable: true, get: function() {
      return errors_1.InternalServerError;
    } });
    Object.defineProperty(exports2, "JobDoesNotExistError", { enumerable: true, get: function() {
      return errors_1.JobDoesNotExistError;
    } });
    Object.defineProperty(exports2, "InvalidPushSettingsError", { enumerable: true, get: function() {
      return errors_1.InvalidPushSettingsError;
    } });
    Object.defineProperty(exports2, "InvocationLimitReachedError", { enumerable: true, get: function() {
      return errors_1.InvocationLimitReachedError;
    } });
    var jobProgress_1 = require_jobProgress();
    Object.defineProperty(exports2, "JobProgress", { enumerable: true, get: function() {
      return jobProgress_1.JobProgress;
    } });
    var queueResponse_1 = require_queueResponse();
    Object.defineProperty(exports2, "QueueResponse", { enumerable: true, get: function() {
      return queueResponse_1.QueueResponse;
    } });
    var invocationError_1 = require_invocationError();
    Object.defineProperty(exports2, "InvocationError", { enumerable: true, get: function() {
      return invocationError_1.InvocationError;
    } });
    var invocationErrorCode_1 = require_invocationErrorCode();
    Object.defineProperty(exports2, "InvocationErrorCode", { enumerable: true, get: function() {
      return invocationErrorCode_1.InvocationErrorCode;
    } });
    var appEvents_1 = require_appEvents();
    Object.defineProperty(exports2, "appEvents", { enumerable: true, get: function() {
      return appEvents_1.appEvents;
    } });
  }
});

// ../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/@forge/kvs/out/entity-query.js
var require_entity_query = __commonJS({
  "../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/@forge/kvs/out/entity-query.js"(exports2) {
    "use strict";
    Object.defineProperty(exports2, "__esModule", { value: true });
    exports2.FilterBuilder = exports2.BaseFilter = exports2.KvsIndexQueryBuilder = void 0;
    var KvsIndexQueryBuilder = class {
      entityName;
      storageApi;
      options;
      constructor(entityName, storageApi, options) {
        this.entityName = entityName;
        this.storageApi = storageApi;
        this.options = options;
      }
      index(name, indexOptions) {
        return new KvsEntityQueryBuilder(this.storageApi, {
          ...this.options,
          entityName: this.entityName,
          indexName: name,
          partition: indexOptions?.partition,
          filters: []
        });
      }
    };
    exports2.KvsIndexQueryBuilder = KvsIndexQueryBuilder;
    var KvsEntityQueryBuilder = class {
      storageApi;
      queryOptions;
      constructor(storageApi, queryOptions) {
        this.storageApi = storageApi;
        this.queryOptions = queryOptions;
      }
      where(condition) {
        this.queryOptions.range = condition;
        return this;
      }
      filters(filter) {
        this.queryOptions.filters = filter.filters();
        this.queryOptions.filterOperator = filter.operator();
        return this;
      }
      sort(sort) {
        this.queryOptions.sort = sort;
        return this;
      }
      cursor(cursor) {
        this.queryOptions.cursor = cursor;
        return this;
      }
      limit(limit) {
        this.queryOptions.limit = limit;
        return this;
      }
      async getOne() {
        const { results } = await this.limit(1).getMany();
        if (results && results.length > 0) {
          return results[0];
        }
        return void 0;
      }
      async getMany() {
        const maybeOptions = {
          ...this.queryOptions?.metadataFields ? { options: { metadataFields: this.queryOptions.metadataFields } } : {}
        };
        const { filters, filterOperator, metadataFields, ...rest } = this.queryOptions;
        if (filters && filterOperator && filters.length > 0) {
          return this.storageApi.queryEntity({
            ...rest,
            ...maybeOptions,
            filters: {
              [filterOperator]: filters
            }
          });
        }
        return this.storageApi.queryEntity({ ...rest, ...maybeOptions });
      }
    };
    var BaseFilter = class {
      items;
      constructor(items = []) {
        this.items = items;
      }
      filters() {
        return this.items;
      }
      operator() {
        return this instanceof AndFilterBuilder ? "and" : "or";
      }
    };
    exports2.BaseFilter = BaseFilter;
    var FilterBuilder = class extends BaseFilter {
      and(field, condition) {
        return new AndFilterBuilder().and(field, condition);
      }
      or(field, condition) {
        return new OrFilterBuilder().or(field, condition);
      }
    };
    exports2.FilterBuilder = FilterBuilder;
    var AndFilterBuilder = class extends BaseFilter {
      and(field, condition) {
        this.items.push({ property: field, ...condition });
        return this;
      }
    };
    var OrFilterBuilder = class extends BaseFilter {
      or(field, condition) {
        this.items.push({ property: field, ...condition });
        return this;
      }
    };
  }
});

// ../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/@forge/kvs/out/entity.js
var require_entity = __commonJS({
  "../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/@forge/kvs/out/entity.js"(exports2) {
    "use strict";
    Object.defineProperty(exports2, "__esModule", { value: true });
    exports2.EntityImpl = void 0;
    var entity_query_1 = require_entity_query();
    var EntityImpl = class {
      entityName;
      storageApi;
      constructor(entityName, storageApi) {
        this.entityName = entityName;
        this.storageApi = storageApi;
      }
      get(key, options) {
        return this.storageApi.getEntity({
          entityName: this.entityName,
          key,
          options
        });
      }
      set(key, value, options) {
        return this.storageApi.setEntity({
          entityName: this.entityName,
          key,
          value,
          options
        });
      }
      delete(key) {
        return this.storageApi.deleteEntity({
          entityName: this.entityName,
          key
        });
      }
      query(options) {
        return new entity_query_1.KvsIndexQueryBuilder(this.entityName, this.storageApi, options);
      }
    };
    exports2.EntityImpl = EntityImpl;
  }
});

// ../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/@forge/kvs/out/query.js
var require_query = __commonJS({
  "../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/@forge/kvs/out/query.js"(exports2) {
    "use strict";
    Object.defineProperty(exports2, "__esModule", { value: true });
    exports2.KvsQueryBuilder = void 0;
    var KvsQueryBuilder = class {
      storageApi;
      options;
      constructor(storageApi, options = {}) {
        this.storageApi = storageApi;
        this.options = options;
      }
      where(property, condition) {
        this.options.where = [{ property, ...condition }];
        return this;
      }
      cursor(cursor) {
        this.options.cursor = cursor;
        return this;
      }
      limit(limit) {
        this.options.limit = limit;
        return this;
      }
      async getOne() {
        const { results } = await this.limit(1).getMany();
        if (results && results.length > 0) {
          return results[0];
        }
        return void 0;
      }
      getMany() {
        const maybeOptions = {
          ...this.options?.metadataFields ? { options: { metadataFields: this.options.metadataFields } } : {}
        };
        return this.storageApi.query({
          limit: this.options.limit,
          after: this.options.cursor,
          where: this.options.where,
          ...maybeOptions
        });
      }
    };
    exports2.KvsQueryBuilder = KvsQueryBuilder;
  }
});

// ../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/@forge/kvs/package.json
var require_package = __commonJS({
  "../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/@forge/kvs/package.json"(exports2, module2) {
    module2.exports = {
      name: "@forge/kvs",
      version: "2.0.7",
      description: "Forge Key Value Store SDK",
      author: "Atlassian",
      license: "SEE LICENSE IN LICENSE.txt",
      main: "out/index.js",
      types: "out/index.d.ts",
      scripts: {
        build: "yarn run clean && yarn run compile",
        clean: "rm -rf ./out && rm -f tsconfig.tsbuildinfo",
        compile: "tsc -b -v"
      },
      devDependencies: {
        "@atlassian/xen-test-util": "^4.2.0",
        "@types/node": "20.19.1",
        typescript: "5.9.2"
      },
      dependencies: {
        "@forge/api": "^8.2.0"
      },
      publishConfig: {
        registry: "https://packages.atlassian.com/api/npm/npm-public/"
      },
      peerDependencies: {
        typescript: ">=5.0.0"
      },
      peerDependenciesMeta: {
        typescript: {
          optional: true
        }
      }
    };
  }
});

// ../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/@forge/kvs/out/version.js
var require_version = __commonJS({
  "../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/@forge/kvs/out/version.js"(exports2) {
    "use strict";
    Object.defineProperty(exports2, "__esModule", { value: true });
    exports2.PACKAGE_VERSION = exports2.PACKAGE_NAME = void 0;
    var packageInfo = require_package();
    exports2.PACKAGE_NAME = packageInfo.name;
    exports2.PACKAGE_VERSION = packageInfo.version;
  }
});

// ../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/@forge/kvs/out/errors.js
var require_errors3 = __commonJS({
  "../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/@forge/kvs/out/errors.js"(exports2) {
    "use strict";
    Object.defineProperty(exports2, "__esModule", { value: true });
    exports2.ForgeKvsAPIError = exports2.ForgeKvsError = void 0;
    var version_1 = require_version();
    var ForgeKvsError = class extends Error {
      packageVersion;
      constructor(message, packageVersion = version_1.PACKAGE_VERSION) {
        super(message);
        this.name = "ForgeKvsError";
        this.packageVersion = packageVersion;
      }
    };
    exports2.ForgeKvsError = ForgeKvsError;
    var ForgeKvsAPIError = class extends ForgeKvsError {
      responseDetails;
      code;
      message;
      context;
      constructor(responseDetails, forgeError) {
        super(forgeError.message);
        const { status, statusText, traceId, httpMethod, httpPath, responseBodyLength } = responseDetails;
        this.responseDetails = { status, statusText, traceId, httpMethod, httpPath, responseBodyLength };
        const { code, message, context, ...bodyData } = forgeError;
        this.code = code;
        this.message = message;
        this.context = { ...context, ...bodyData };
      }
    };
    exports2.ForgeKvsAPIError = ForgeKvsAPIError;
  }
});

// ../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/@forge/kvs/out/utils/transaction-request-builder.js
var require_transaction_request_builder = __commonJS({
  "../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/@forge/kvs/out/utils/transaction-request-builder.js"(exports2) {
    "use strict";
    Object.defineProperty(exports2, "__esModule", { value: true });
    exports2.buildRequestChecks = exports2.buildRequestDeletes = exports2.buildRequestSet = void 0;
    var errors_1 = require_errors3();
    function buildConditionsRequest(filter) {
      if (!filter) {
        return void 0;
      }
      if (filter.filters().length === 0) {
        throw new errors_1.ForgeKvsError("Builder must have at least one condition set");
      }
      return {
        [filter.operator()]: filter.filters()
      };
    }
    var buildRequestSet = (setOperation) => {
      const { key, value, entity, options } = setOperation;
      const entityName = entity?.entityName;
      const conditions = buildConditionsRequest(entity?.conditions);
      return {
        key,
        value,
        entityName,
        conditions,
        options
      };
    };
    exports2.buildRequestSet = buildRequestSet;
    var buildRequestDeletes = (deleteOperation) => {
      const { key, entity } = deleteOperation;
      const entityName = entity?.entityName;
      const conditions = buildConditionsRequest(entity?.conditions);
      return {
        key,
        entityName,
        conditions
      };
    };
    exports2.buildRequestDeletes = buildRequestDeletes;
    var buildRequestChecks = (checkOperation) => {
      const { key, entity } = checkOperation;
      const entityName = entity.entityName;
      const conditions = buildConditionsRequest(entity.conditions);
      return {
        key,
        entityName,
        conditions
      };
    };
    exports2.buildRequestChecks = buildRequestChecks;
  }
});

// ../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/@forge/kvs/out/transaction-api.js
var require_transaction_api = __commonJS({
  "../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/@forge/kvs/out/transaction-api.js"(exports2) {
    "use strict";
    Object.defineProperty(exports2, "__esModule", { value: true });
    exports2.TransactionBuilderImpl = void 0;
    var transaction_request_builder_1 = require_transaction_request_builder();
    var TransactionBuilderImpl = class {
      storageApi;
      sets;
      deletes;
      checks;
      constructor(storageApi, sets = [], deletes = [], checks = []) {
        this.storageApi = storageApi;
        this.sets = sets;
        this.deletes = deletes;
        this.checks = checks;
      }
      set(key, value, entity, options) {
        const transactSet = {
          key,
          value,
          options
        };
        if (entity) {
          transactSet.entity = {
            entityName: entity.entityName,
            conditions: entity.conditions
          };
        }
        this.sets.push(transactSet);
        return this;
      }
      delete(key, entity) {
        const transactDelete = {
          key
        };
        if (entity) {
          transactDelete.entity = {
            entityName: entity.entityName,
            conditions: entity.conditions
          };
        }
        this.deletes.push(transactDelete);
        return this;
      }
      check(key, { entityName, conditions }) {
        const transactCheck = {
          key,
          entity: {
            entityName,
            conditions
          }
        };
        this.checks.push(transactCheck);
        return this;
      }
      async execute() {
        const undefineEmptyArray = (arr) => {
          return arr.length === 0 ? void 0 : arr;
        };
        const request = {
          set: undefineEmptyArray(this.sets.map(transaction_request_builder_1.buildRequestSet)),
          delete: undefineEmptyArray(this.deletes.map(transaction_request_builder_1.buildRequestDeletes)),
          check: undefineEmptyArray(this.checks.map(transaction_request_builder_1.buildRequestChecks))
        };
        await this.storageApi.transact(request);
      }
    };
    exports2.TransactionBuilderImpl = TransactionBuilderImpl;
  }
});

// ../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/@forge/kvs/out/kvs.js
var require_kvs = __commonJS({
  "../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/@forge/kvs/out/kvs.js"(exports2) {
    "use strict";
    Object.defineProperty(exports2, "__esModule", { value: true });
    exports2.KvsImpl = void 0;
    var entity_1 = require_entity();
    var query_1 = require_query();
    var transaction_api_1 = require_transaction_api();
    var KvsImpl = class {
      storageApi;
      constructor(storageApi) {
        this.storageApi = storageApi;
      }
      get(key, options) {
        return this.storageApi.get({ key, options });
      }
      set(key, value, options) {
        return this.storageApi.set({ key, value, options });
      }
      batchSet(items) {
        return this.storageApi.batchSet(items);
      }
      batchDelete(items) {
        return this.storageApi.batchDelete(items);
      }
      batchGet(items) {
        return this.storageApi.batchGet(items);
      }
      delete(key) {
        return this.storageApi.delete({ key });
      }
      query(options) {
        return new query_1.KvsQueryBuilder(this.storageApi, options);
      }
      getSecret(key, options) {
        return this.storageApi.getSecret({ key, options });
      }
      setSecret(key, value, options) {
        return this.storageApi.setSecret({ key, value, options });
      }
      deleteSecret(key) {
        return this.storageApi.deleteSecret({ key });
      }
      entity(entityName) {
        return new entity_1.EntityImpl(entityName, this.storageApi);
      }
      transact() {
        return new transaction_api_1.TransactionBuilderImpl(this.storageApi);
      }
    };
    exports2.KvsImpl = KvsImpl;
  }
});

// ../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/@forge/kvs/out/interfaces/types.js
var require_types2 = __commonJS({
  "../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/@forge/kvs/out/interfaces/types.js"(exports2) {
    "use strict";
    Object.defineProperty(exports2, "__esModule", { value: true });
    exports2.Sort = exports2.MetadataField = void 0;
    exports2.isOverrideAndReturnOptions = isOverrideAndReturnOptions;
    var MetadataField;
    (function(MetadataField2) {
      MetadataField2["CREATED_AT"] = "CREATED_AT";
      MetadataField2["UPDATED_AT"] = "UPDATED_AT";
      MetadataField2["EXPIRE_TIME"] = "EXPIRE_TIME";
    })(MetadataField || (exports2.MetadataField = MetadataField = {}));
    function isOverrideAndReturnOptions(options) {
      return options !== void 0 && "returnValue" in options;
    }
    var Sort2;
    (function(Sort3) {
      Sort3["ASC"] = "ASC";
      Sort3["DESC"] = "DESC";
    })(Sort2 || (exports2.Sort = Sort2 = {}));
  }
});

// ../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/@forge/kvs/out/utils/error-handling.js
var require_error_handling = __commonJS({
  "../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/@forge/kvs/out/utils/error-handling.js"(exports2) {
    "use strict";
    Object.defineProperty(exports2, "__esModule", { value: true });
    exports2.isForgeError = isForgeError;
    exports2.safeGetParsedBody = safeGetParsedBody;
    exports2.getAPIErrorResponseDetails = getAPIErrorResponseDetails;
    exports2.extractTraceId = extractTraceId;
    exports2.checkResponseError = checkResponseError;
    var errors_1 = require_errors3();
    function isForgeError(body) {
      return typeof body === "object" && body !== null && "code" in body && "message" in body;
    }
    function safeGetParsedBody(text) {
      try {
        return JSON.parse(text);
      } catch (error) {
        return void 0;
      }
    }
    function getAPIErrorResponseDetails(response, responseText, requestContext) {
      return {
        status: response.status,
        statusText: response.statusText,
        traceId: extractTraceId(response),
        httpMethod: requestContext?.httpMethod,
        httpPath: requestContext?.httpPath,
        responseBodyLength: responseText.length
      };
    }
    function extractTraceId(response) {
      return response.headers.get("x-b3-traceid") || response.headers.get("x-trace-id");
    }
    async function checkResponseError(response, requestContext) {
      if (response.ok) {
        return;
      }
      const responseText = await response.text();
      const details = getAPIErrorResponseDetails(response, responseText, requestContext);
      const parsedBody = safeGetParsedBody(responseText);
      if (parsedBody && isForgeError(parsedBody)) {
        throw new errors_1.ForgeKvsAPIError(details, parsedBody);
      }
      throw new errors_1.ForgeKvsAPIError(details, {
        code: "UNKNOWN_ERROR",
        message: "Unexpected error in Forge KVS API",
        context: { responseText }
      });
    }
  }
});

// ../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/@forge/kvs/out/storage-api.js
var require_storage_api = __commonJS({
  "../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/@forge/kvs/out/storage-api.js"(exports2) {
    "use strict";
    Object.defineProperty(exports2, "__esModule", { value: true });
    exports2.StorageApi = void 0;
    var types_1 = require_types2();
    var error_handling_1 = require_error_handling();
    var errors_1 = require_errors3();
    var ResponseType;
    (function(ResponseType2) {
      ResponseType2[ResponseType2["NONE"] = 0] = "NONE";
      ResponseType2[ResponseType2["EXPECTED"] = 1] = "EXPECTED";
      ResponseType2[ResponseType2["OPTIONAL"] = 2] = "OPTIONAL";
    })(ResponseType || (ResponseType = {}));
    var StorageApi = class {
      apiClient;
      constructor(apiClient) {
        this.apiClient = apiClient;
      }
      async get(body) {
        const rs = await this.handleKeyNotFound(async () => {
          return this.request("/api/v1/get", body, ResponseType.EXPECTED);
        });
        return this.processGetResponse(rs, body.options);
      }
      async getSecret(body) {
        const rs = await this.handleKeyNotFound(async () => {
          return this.request("/api/v1/secret/get", body, ResponseType.EXPECTED);
        });
        return this.processGetResponse(rs, body.options);
      }
      async getEntity(body) {
        const rs = await this.handleKeyNotFound(async () => {
          return this.request("/api/v1/entity/get", body, ResponseType.EXPECTED);
        });
        return this.processGetResponse(rs, body.options);
      }
      async set(body) {
        const rs = await this.request("/api/v1/set", body, ResponseType.OPTIONAL);
        return rs && (0, types_1.isOverrideAndReturnOptions)(body.options) ? this.processSetResponse(rs, body.options) : void 0;
      }
      async setSecret(body) {
        const rs = await this.request("/api/v1/secret/set", body, ResponseType.OPTIONAL);
        return rs && (0, types_1.isOverrideAndReturnOptions)(body.options) ? this.processSetResponse(rs, body.options) : void 0;
      }
      async setEntity(body) {
        const rs = await this.request("/api/v1/entity/set", body, ResponseType.OPTIONAL);
        return rs && (0, types_1.isOverrideAndReturnOptions)(body.options) ? this.processSetResponse(rs, body.options) : void 0;
      }
      async delete(body) {
        await this.handleKeyNotFound(async () => {
          return this.request("/api/v1/delete", body, ResponseType.NONE);
        });
      }
      async deleteSecret(body) {
        await this.handleKeyNotFound(async () => {
          return this.request("/api/v1/secret/delete", body, ResponseType.NONE);
        });
      }
      async deleteEntity(body) {
        await this.handleKeyNotFound(async () => {
          return this.request("/api/v1/entity/delete", body, ResponseType.NONE);
        });
      }
      async query(body) {
        const rs = await this.request("/api/v1/query", body, ResponseType.EXPECTED);
        return {
          results: rs.data,
          nextCursor: rs.cursor
        };
      }
      async queryEntity(body) {
        const rs = await this.request("/api/v1/entity/query", body, ResponseType.EXPECTED);
        return {
          results: rs.data,
          nextCursor: rs.cursor
        };
      }
      async batchSet(body) {
        const rs = await this.request("/api/v1/batch/set", body, ResponseType.EXPECTED);
        return {
          successfulKeys: rs.successfulKeys,
          failedKeys: rs.failedKeys
        };
      }
      async batchDelete(body) {
        const rs = await this.request("/api/v1/batch/delete", body, ResponseType.EXPECTED);
        return {
          successfulKeys: rs.successfulKeys,
          failedKeys: rs.failedKeys
        };
      }
      async batchGet(body) {
        const rs = await this.request("/api/v1/batch/get", body, ResponseType.EXPECTED);
        return {
          successfulKeys: rs.successfulKeys,
          failedKeys: rs.failedKeys
        };
      }
      async transact(transactionRequest) {
        await this.request("/api/v1/transaction", transactionRequest, ResponseType.NONE);
      }
      async handleKeyNotFound(fn) {
        try {
          return await fn();
        } catch (e) {
          if (e instanceof errors_1.ForgeKvsAPIError && e.code === "KEY_NOT_FOUND") {
            return void 0;
          }
          throw e;
        }
      }
      async request(path, body, responseType) {
        const requestBody = {
          method: "POST",
          body: JSON.stringify(body),
          headers: {
            "content-type": "application/json"
          }
        };
        const response = await this.apiClient(path, requestBody);
        const requestContext = {
          httpMethod: requestBody.method,
          httpPath: path
        };
        await (0, error_handling_1.checkResponseError)(response, requestContext);
        if (responseType === ResponseType.NONE) {
          return;
        }
        const responseText = await response.text();
        if (responseType === ResponseType.OPTIONAL && !responseText) {
          return void 0;
        }
        const parsedBody = (0, error_handling_1.safeGetParsedBody)(responseText);
        if (parsedBody === void 0) {
          const details = (0, error_handling_1.getAPIErrorResponseDetails)(response, responseText, requestContext);
          throw new errors_1.ForgeKvsAPIError(details, {
            code: "UNKNOWN_ERROR",
            message: "Unexpected error in Forge KVS API. Response was not valid JSON",
            context: { contentLength: response.headers.get("content-length") }
          });
        }
        return parsedBody;
      }
      processGetResponse(response, options) {
        if (response && options) {
          const maybeCreatedAt = options.metadataFields?.includes(types_1.MetadataField.CREATED_AT) ? { createdAt: response.createdAt } : {};
          const maybeUpdatedAt = options.metadataFields?.includes(types_1.MetadataField.UPDATED_AT) ? { updatedAt: response.updatedAt } : {};
          const maybeExpireTime = options.metadataFields?.includes(types_1.MetadataField.EXPIRE_TIME) ? { expireTime: response.expireTime } : {};
          return {
            key: response.key,
            value: response.value,
            ...maybeCreatedAt,
            ...maybeUpdatedAt,
            ...maybeExpireTime
          };
        }
        return response?.value;
      }
      processSetResponse(response, options) {
        if (!response) {
          return void 0;
        }
        return {
          key: response.key,
          value: response.value,
          ...options.returnMetadataFields?.includes(types_1.MetadataField.CREATED_AT) && { createdAt: response.createdAt },
          ...options.returnMetadataFields?.includes(types_1.MetadataField.UPDATED_AT) && { updatedAt: response.updatedAt },
          ...options.returnMetadataFields?.includes(types_1.MetadataField.EXPIRE_TIME) && { expireTime: response.expireTime }
        };
      }
    };
    exports2.StorageApi = StorageApi;
  }
});

// ../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/@forge/kvs/out/conditions.js
var require_conditions = __commonJS({
  "../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/@forge/kvs/out/conditions.js"(exports2) {
    "use strict";
    Object.defineProperty(exports2, "__esModule", { value: true });
    exports2.FilterConditions = exports2.WhereConditions = void 0;
    function between(firstValue, secondValue) {
      return {
        condition: "BETWEEN",
        values: [firstValue, secondValue]
      };
    }
    function beginsWith(value) {
      return {
        condition: "BEGINS_WITH",
        values: [value]
      };
    }
    function exists() {
      return {
        condition: "EXISTS",
        values: [true]
      };
    }
    function notExists() {
      return {
        condition: "NOT_EXISTS",
        values: [true]
      };
    }
    function greaterThan(value) {
      return {
        condition: "GREATER_THAN",
        values: [value]
      };
    }
    function greaterThanEqualTo(value) {
      return {
        condition: "GREATER_THAN_EQUAL_TO",
        values: [value]
      };
    }
    function lessThan(value) {
      return {
        condition: "LESS_THAN",
        values: [value]
      };
    }
    function lessThanEqualTo(value) {
      return {
        condition: "LESS_THAN_EQUAL_TO",
        values: [value]
      };
    }
    function contains(value) {
      return {
        condition: "CONTAINS",
        values: [value]
      };
    }
    function notContains(value) {
      return {
        condition: "NOT_CONTAINS",
        values: [value]
      };
    }
    function equalTo(value) {
      return {
        condition: "EQUAL_TO",
        values: [value]
      };
    }
    function notEqualTo(value) {
      return {
        condition: "NOT_EQUAL_TO",
        values: [value]
      };
    }
    exports2.WhereConditions = {
      beginsWith,
      between,
      equalTo,
      greaterThan,
      greaterThanEqualTo,
      lessThan,
      lessThanEqualTo
    };
    exports2.FilterConditions = {
      beginsWith,
      between,
      contains,
      notContains,
      equalTo,
      notEqualTo,
      exists,
      notExists,
      greaterThan,
      greaterThanEqualTo,
      lessThan,
      lessThanEqualTo
    };
  }
});

// ../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/@forge/kvs/out/index.js
var require_out6 = __commonJS({
  "../../../../../../Library/Application Support/Goose/benchmark/forge-kit/0a0b8c760fb127e6/app-modules/node_modules/@forge/kvs/out/index.js"(exports2) {
    "use strict";
    Object.defineProperty(exports2, "__esModule", { value: true });
    exports2.kvs = exports2.Filter = exports2.WhereConditions = exports2.FilterConditions = void 0;
    var tslib_1 = (init_tslib_es6(), __toCommonJS(tslib_es6_exports));
    var kvs_1 = require_kvs();
    var storage_api_1 = require_storage_api();
    function getFetchClient() {
      const runtime = global.__forge_runtime__;
      if (runtime?.kvs?.url && runtime?.kvs?.host) {
        const { proxy, kvs: kvs3, tracing } = runtime;
        return async function(path, options) {
          return await global.__forge_fetch__({ type: "kvs" }, path, {
            ...options,
            headers: {
              ...options?.headers,
              Authorization: `Bearer ${proxy.token}`,
              Host: kvs3?.host,
              "x-b3-traceid": tracing.traceId,
              "x-b3-spanid": tracing.spanId
            }
          });
        };
      }
      return async function(path, options) {
        return await global.__forge_fetch__({
          type: "kvs",
          provider: "app",
          remote: "kvs"
        }, path, options);
      };
    }
    var storageApi = new storage_api_1.StorageApi(getFetchClient());
    var kvs2 = new kvs_1.KvsImpl(storageApi);
    exports2.kvs = kvs2;
    var conditions_1 = require_conditions();
    Object.defineProperty(exports2, "FilterConditions", { enumerable: true, get: function() {
      return conditions_1.FilterConditions;
    } });
    Object.defineProperty(exports2, "WhereConditions", { enumerable: true, get: function() {
      return conditions_1.WhereConditions;
    } });
    var entity_query_1 = require_entity_query();
    Object.defineProperty(exports2, "Filter", { enumerable: true, get: function() {
      return entity_query_1.FilterBuilder;
    } });
    tslib_1.__exportStar(require_errors3(), exports2);
    tslib_1.__exportStar(require_types2(), exports2);
    exports2.default = kvs2;
  }
});

// src/backend.js
var backend_exports = {};
__export(backend_exports, {
  consume: () => consume,
  hourly: () => hourly,
  onIssueUpdated: () => onIssueUpdated
});
module.exports = __toCommonJS(backend_exports);
var import_events = __toESM(require_out5());

// src/jira.js
var import_api = __toESM(require_out4());
var RetryLater = class extends Error {
  constructor(seconds) {
    super(`Jira asked to retry after ${seconds}s`);
    this.seconds = seconds;
  }
};
var sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
function retryAfterSeconds(res) {
  const raw = res.headers.get("retry-after");
  if (!raw) return 1;
  const n = Number(raw);
  if (Number.isFinite(n)) return Math.max(n, 0);
  const at = Date.parse(raw);
  return Number.isFinite(at) ? Math.max((at - Date.now()) / 1e3, 0) : 1;
}
function makeClient(principal, { waitBudgetSeconds }) {
  let waited = 0;
  async function send(r, init) {
    for (; ; ) {
      const res = await principal.requestJira(r, init);
      if (res.status !== 429) return res;
      const seconds = retryAfterSeconds(res);
      if (waited + seconds > waitBudgetSeconds) throw new RetryLater(seconds);
      waited += seconds;
      await sleep(Math.ceil(seconds * 1e3) + 50);
    }
  }
  async function json(r, init) {
    const res = await send(r, init);
    if (res.status === 404) return null;
    if (!res.ok) throw new Error(`Jira ${res.status} on ${r.value || r}: ${(await res.text()).slice(0, 300)}`);
    return res.status === 204 ? {} : res.json();
  }
  return { send, json };
}
var appClient = (opts) => makeClient(import_api.default.asApp(), opts);
async function scrumBoards(jira) {
  const boards = [];
  for (let startAt = 0; ; ) {
    const page = await jira.json(import_api.route`/rest/agile/1.0/board?type=scrum&startAt=${startAt}&maxResults=50`);
    for (const b of page.values || []) boards.push({ id: String(b.id), name: b.name, projectId: b.location && b.location.projectId != null ? String(b.location.projectId) : null });
    if (page.isLast || !(page.values || []).length) break;
    startAt += page.values.length;
  }
  return boards;
}
async function boardSprints(jira, boardId, state) {
  const sprints = [];
  for (let startAt = 0; ; ) {
    const page = await jira.json(import_api.route`/rest/agile/1.0/board/${boardId}/sprint?state=${state}&startAt=${startAt}&maxResults=50`);
    if (!page) break;
    sprints.push(...page.values || []);
    if (page.isLast || !(page.values || []).length) break;
    startAt += page.values.length;
  }
  return sprints;
}
async function boardEstimationField(jira, boardId) {
  const cfg = await jira.json(import_api.route`/rest/agile/1.0/board/${boardId}/configuration`);
  const est = cfg && cfg.estimation;
  return est && est.type === "field" && est.field ? est.field.fieldId : null;
}
var getSprint = (jira, sprintId) => jira.json(import_api.route`/rest/agile/1.0/sprint/${sprintId}`);
async function sprintFieldId(jira) {
  const fields = await jira.json(import_api.route`/rest/api/3/field`);
  const f = fields.find((x) => x.schema && x.schema.custom === "com.pyxis.greenhopper.jira:gh-sprint");
  if (!f) throw new Error("This site has no Sprint field (com.pyxis.greenhopper.jira:gh-sprint)");
  return f.id;
}
async function completeChangelog(jira, issue) {
  const cl = issue.changelog || { histories: [], total: 0 };
  const histories = [...cl.histories || []];
  if ((cl.total || 0) > histories.length) {
    histories.length = 0;
    for (let startAt = 0; ; ) {
      const page = await jira.json(import_api.route`/rest/api/3/issue/${issue.id}/changelog?startAt=${startAt}&maxResults=100`);
      histories.push(...page.values || []);
      if (page.isLast || !(page.values || []).length) break;
      startAt += page.values.length;
    }
  }
  return { ...issue, changelog: { histories } };
}
async function issueWithChangelog(jira, issueIdOrKey, fields) {
  const issue = await jira.json(import_api.route`/rest/api/3/issue/${issueIdOrKey}?fields=${fields.join(",")}&expand=changelog`);
  return issue ? completeChangelog(jira, issue) : null;
}
async function sprintIssues(jira, sprintId, fields) {
  const issues = [];
  for (let startAt = 0; ; ) {
    const page = await jira.json(import_api.route`/rest/software/1.0/sprint/${sprintId}/issue?fields=${fields.join(",")}&expand=changelog&startAt=${startAt}&maxResults=100`);
    if (!page) break;
    for (const i of page.issues || []) issues.push(await completeChangelog(jira, i));
    const n = (page.issues || []).length;
    if (!n || startAt + n >= (page.total || 0)) break;
    startAt += n;
  }
  return issues;
}
async function searchIssues(jira, jql, fields) {
  const issues = [];
  let token;
  do {
    const page = token ? await jira.json(import_api.route`/rest/api/3/search/jql?jql=${jql}&fields=${fields.join(",")}&expand=changelog&maxResults=100&nextPageToken=${token}`) : await jira.json(import_api.route`/rest/api/3/search/jql?jql=${jql}&fields=${fields.join(",")}&expand=changelog&maxResults=100`);
    for (const i of page.issues || []) issues.push(await completeChangelog(jira, i));
    token = page.isLast ? void 0 : page.nextPageToken;
  } while (token);
  return issues;
}

// src/store.js
var import_kvs = __toESM(require_out6());
var CHANGE = "scope-change";
var MEMBER = "scope-member";
var REGISTRY = "registry";
var changeKey = (sprintId, changeId) => `${sprintId}#${changeId}`;
var memberKey = (sprintId, issueId) => `${sprintId}#${issueId}`;
async function readIndex(entity, index, sprintId) {
  const rows = [];
  let cursor;
  do {
    let q = import_kvs.default.entity(entity).query().index(index, { partition: [String(sprintId)] }).sort(import_kvs.Sort.ASC).limit(100);
    if (cursor) q = q.cursor(cursor);
    const page = await q.getMany();
    for (const r of page.results) rows.push(r.value);
    cursor = page.nextCursor;
  } while (cursor);
  return rows;
}
var sprintChanges = (sprintId) => readIndex(CHANGE, "sprint-time", sprintId);
var sprintMembers = (sprintId) => readIndex(MEMBER, "sprint-issue", sprintId);
var getRegistry = () => import_kvs.default.get(REGISTRY);
var setRegistry = (value) => import_kvs.default.set(REGISTRY, value);
async function recordChange(row) {
  try {
    await import_kvs.default.entity(CHANGE).set(changeKey(row.sprintId, row.changeId), row, { keyPolicy: "FAIL_IF_EXISTS" });
    return true;
  } catch (err) {
    if (/exist|CONDITIONAL|DUPLICATE/i.test(`${err && err.code} ${err && err.message}`)) return false;
    throw err;
  }
}
var getMember = (sprintId, issueId) => import_kvs.default.entity(MEMBER).get(memberKey(sprintId, issueId));
var putMember = (row) => import_kvs.default.entity(MEMBER).set(memberKey(row.sprintId, row.issueId), row);
var getChange = (sprintId, changeId) => import_kvs.default.entity(CHANGE).get(changeKey(sprintId, changeId));

// src/model.js
function sprintIdList(raw) {
  if (raw == null || raw === "") return [];
  return String(raw).split(",").map((s) => s.trim()).filter(Boolean);
}
var byTimeThenId = (a, b) => a.atMs - b.atMs || Number(a.changeId) - Number(b.changeId);
function sprintEntries(issue, sprintFieldId2) {
  const out = [];
  for (const h of issue.changelog && issue.changelog.histories || []) {
    const atMs = Date.parse(h.created);
    for (const item of h.items || []) {
      if (item.fieldId !== sprintFieldId2 && !(item.fieldId == null && item.field === "Sprint")) continue;
      out.push({
        changeId: String(h.id),
        atMs,
        from: sprintIdList(item.from),
        to: sprintIdList(item.to),
        authorId: h.author ? h.author.accountId : null,
        authorName: h.author ? h.author.displayName : "Unknown"
      });
    }
  }
  return out.sort(byTimeThenId);
}
function currentSprintIds(issue, sprintFieldId2) {
  const v = issue.fields && issue.fields[sprintFieldId2];
  return (Array.isArray(v) ? v : []).map((s) => String(s && typeof s === "object" ? s.id : s));
}
function estimateOf(issue, fieldId) {
  const v = fieldId && issue.fields ? issue.fields[fieldId] : null;
  const n = typeof v === "number" ? v : Number(v);
  return v == null || v === "" || !Number.isFinite(n) ? 0 : n;
}
function assess(issue, sprint, sprintFieldId2) {
  const S = String(sprint.id);
  const startMs = Date.parse(sprint.startDate);
  const entries = sprintEntries(issue, sprintFieldId2);
  const after = entries.filter((e) => e.atMs > startMs);
  const inNow = currentSprintIds(issue, sprintFieldId2).includes(S);
  const createdMs = Date.parse(issue.fields && issue.fields.created);
  let atStart;
  if (Number.isFinite(createdMs) && createdMs > startMs) atStart = false;
  else if (after.length) atStart = after[0].from.includes(S);
  else atStart = inNow;
  const seen = /* @__PURE__ */ new Set();
  const changes = [];
  for (const e of after) {
    const wasIn = e.from.includes(S);
    const isIn = e.to.includes(S);
    if (wasIn === isIn || seen.has(e.changeId)) continue;
    seen.add(e.changeId);
    changes.push({
      changeId: e.changeId,
      atMs: e.atMs,
      kind: isIn ? "added" : "removed",
      authorId: e.authorId,
      authorName: e.authorName
    });
  }
  const everIn = atStart || inNow || changes.some((c) => c.kind === "added");
  return { atStart, inNow, everIn, changes };
}

// src/ledger.js
function issueFields(registry) {
  const est = new Set(Object.values(registry.boards).map((b) => b.estField).filter(Boolean));
  return [registry.sprintField, "created", "project", ...est];
}
function estimateFieldOf(registry, sprint) {
  const board = registry.boards[String(sprint.originBoardId)];
  return board ? board.estField : null;
}
function sprintsTouching(issue, sprintFieldId2) {
  const ids = new Set(currentSprintIds(issue, sprintFieldId2));
  for (const e of sprintEntries(issue, sprintFieldId2)) for (const id of [...e.from, ...e.to]) ids.add(id);
  return ids;
}
var isTracked = (s) => s && s.state === "active" && !!s.startDate;
async function learnSprints(jira, registry, ids) {
  let changed = false;
  for (const id of ids) {
    if (registry.sprints[id]) continue;
    const s = await getSprint(jira, id);
    if (!s) continue;
    registry.sprints[id] = { id: String(s.id), name: s.name, state: s.state, startDate: s.startDate || null, originBoardId: s.originBoardId != null ? String(s.originBoardId) : null };
    const boardId = registry.sprints[id].originBoardId;
    if (boardId && !registry.boards[boardId]) registry.boards[boardId] = { name: null, estField: await boardEstimationField(jira, boardId) };
    changed = true;
  }
  return changed;
}
function rowsForIssue(issue, registry, source) {
  const members = [];
  const changes = [];
  const projectId = issue.fields && issue.fields.project ? String(issue.fields.project.id) : null;
  for (const id of sprintsTouching(issue, registry.sprintField)) {
    const sprint = registry.sprints[id];
    if (!isTracked(sprint)) continue;
    const a = assess(issue, sprint, registry.sprintField);
    if (!a.everIn && !a.atStart) continue;
    members.push({
      sprintId: sprint.id,
      issueId: String(issue.id),
      issueNum: Number(issue.id),
      issueKey: issue.key,
      projectId,
      points: estimateOf(issue, estimateFieldOf(registry, sprint)),
      atStart: a.atStart,
      inNow: a.inNow,
      everIn: a.everIn
    });
    for (const c of a.changes) {
      changes.push({
        sprintId: sprint.id,
        changeId: c.changeId,
        changeNum: Number(c.changeId),
        atMs: c.atMs,
        at: new Date(c.atMs).toISOString(),
        issueId: String(issue.id),
        issueKey: issue.key,
        projectId,
        kind: c.kind,
        authorId: c.authorId,
        authorName: c.authorName,
        source
      });
    }
  }
  return { members, changes };
}
var MEMBER_FIELDS = ["issueKey", "projectId", "points", "atStart", "inNow", "everIn"];
var memberDiffers = (a, b) => !a || MEMBER_FIELDS.some((f) => a[f] !== b[f]);

// src/backend.js
var work = new import_events.Queue({ key: "scope-ledger-work" });
var WRITE_CHUNK = 40;
function watchedFields(registry) {
  if (!registry) return null;
  return new Set([registry.sprintField, ...Object.values(registry.boards).map((b) => b.estField)].filter(Boolean));
}
async function onIssueUpdated(event) {
  const items = event && event.changelog && event.changelog.items || [];
  const watched = watchedFields(await getRegistry());
  const relevant = items.some(
    (i) => watched ? watched.has(i.fieldId) : i.field === "Sprint" || /story point|estimate/i.test(i.field || "")
  );
  if (!relevant || !event.issue) return { queued: false };
  await work.push([{ body: { kind: "issue", issueId: String(event.issue.id) } }]);
  return { queued: true };
}
async function writeRows({ members, changes }) {
  let written = 0;
  for (const m of members) {
    if (memberDiffers(await getMember(m.sprintId, m.issueId), m)) {
      await putMember(m);
      written += 1;
    }
  }
  for (const c of changes) {
    if (!await getChange(c.sprintId, c.changeId) && await recordChange(c)) written += 1;
  }
  return written;
}
async function syncOneIssue(issueId) {
  const jira = appClient({ waitBudgetSeconds: 20 });
  let registry = await getRegistry();
  if (!registry) registry = { sprintField: await sprintFieldId(jira), boards: {}, sprints: {} };
  const fields = issueFields(registry);
  let issue = await issueWithChangelog(jira, issueId, fields);
  if (!issue) return { written: 0, reason: "issue not found" };
  if (await learnSprints(jira, registry, sprintsTouching(issue, registry.sprintField))) {
    await setRegistry(registry);
    if (issueFields(registry).some((f) => !fields.includes(f))) issue = await issueWithChangelog(jira, issueId, issueFields(registry));
  }
  return { written: await writeRows(rowsForIssue(issue, registry, "event")) };
}
async function consume(event) {
  const body = event && event.body || {};
  try {
    if (body.kind === "issue") return await syncOneIssue(body.issueId);
    if (body.kind === "write") return { written: await writeRows(body) };
    return { ignored: body.kind || null };
  } catch (err) {
    if (err instanceof RetryLater) {
      return new import_events.InvocationError({
        retryAfter: Math.max(1, Math.ceil(err.seconds)),
        retryReason: import_events.InvocationErrorCode.FUNCTION_UPSTREAM_RATE_LIMITED,
        retryData: { kind: body.kind, issueId: body.issueId || null }
      });
    }
    throw err;
  }
}
function jqlInstant(ms) {
  const d = new Date(ms);
  const p = (n) => String(n).padStart(2, "0");
  return `${d.getUTCFullYear()}-${p(d.getUTCMonth() + 1)}-${p(d.getUTCDate())} ${p(d.getUTCHours())}:${p(d.getUTCMinutes())}`;
}
async function refreshRegistry(jira, previous) {
  const next = {
    sprintField: previous && previous.sprintField || await sprintFieldId(jira),
    boards: {},
    sprints: {}
  };
  for (const b of await scrumBoards(jira)) {
    next.boards[b.id] = { name: b.name, estField: await boardEstimationField(jira, b.id) };
    for (const s of await boardSprints(jira, b.id, "active")) {
      next.sprints[String(s.id)] = { id: String(s.id), name: s.name, state: s.state, startDate: s.startDate || null, originBoardId: s.originBoardId != null ? String(s.originBoardId) : b.id };
    }
  }
  for (const s of Object.values(next.sprints)) {
    if (s.originBoardId && !next.boards[s.originBoardId]) next.boards[s.originBoardId] = { name: null, estField: await boardEstimationField(jira, s.originBoardId) };
  }
  if (JSON.stringify(previous) !== JSON.stringify(next)) await setRegistry(next);
  return next;
}
async function hourly() {
  const jira = appClient({ waitBudgetSeconds: 600 });
  const registry = await refreshRegistry(jira, await getRegistry());
  const active = Object.values(registry.sprints).filter(isTracked);
  if (!active.length) return { sprints: 0, queued: 0 };
  const fields = issueFields(registry);
  const issues = /* @__PURE__ */ new Map();
  for (const s of active) for (const i of await sprintIssues(jira, s.id, fields)) issues.set(String(i.id), i);
  const since = Math.min(...active.map((s) => Date.parse(s.startDate))) - 24 * 3600 * 1e3;
  const leavers = await searchIssues(jira, `updated >= "${jqlInstant(since)}" AND (sprint not in openSprints() OR sprint is EMPTY)`, fields);
  for (const i of leavers) if (!issues.has(String(i.id))) issues.set(String(i.id), i);
  const known = /* @__PURE__ */ new Map();
  for (const s of active) {
    const [members2, changes2] = await Promise.all([sprintMembers(s.id), sprintChanges(s.id)]);
    known.set(s.id, { members: new Map(members2.map((m) => [m.issueId, m])), changes: new Set(changes2.map((c) => c.changeId)) });
  }
  const members = [];
  const changes = [];
  for (const issue of issues.values()) {
    const rows2 = rowsForIssue(issue, registry, "reconcile");
    for (const m of rows2.members) if (memberDiffers(known.get(m.sprintId).members.get(m.issueId), m)) members.push(m);
    for (const c of rows2.changes) if (!known.get(c.sprintId).changes.has(c.changeId)) changes.push(c);
  }
  const rows = [...members.map((m) => ["m", m]), ...changes.map((c) => ["c", c])];
  const batch = [];
  for (let i = 0; i < rows.length; i += WRITE_CHUNK) {
    const slice = rows.slice(i, i + WRITE_CHUNK);
    batch.push({ body: { kind: "write", members: slice.filter((r) => r[0] === "m").map((r) => r[1]), changes: slice.filter((r) => r[0] === "c").map((r) => r[1]) } });
  }
  for (let i = 0; i < batch.length; i += 50) await work.push(batch.slice(i, i + 50));
  return { sprints: active.length, issues: issues.size, members: members.length, changes: changes.length, queued: batch.length };
}
// Annotate the CommonJS export names for ESM import in node:
0 && (module.exports = {
  consume,
  hourly,
  onIssueUpdated
});
