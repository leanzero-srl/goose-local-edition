'use strict';
// The Content-Security-Policy a Custom UI resource is served with, computed by Atlassian's own
// @forge/csp 6.3.1 (the package @forge/lint and the tunnel use): CSPProcessingService reads the
// resource's index.html and the manifest's permissions (content.styles/scripts, external.*), and
// CSPInjectionService.getInjectableCSP builds the production ('prod') header around it. Two
// harness substitutions, both stated: `report-uri` points at the host's local reporter instead of
// web-security-reports.services.atlassian.com (no internet), and `hostname` is the site URL.
// Note (production behaviour the library encodes): static inline <script> blocks present in the
// committed index.html are hashed into script-src unless `content.scripts` declares unsafe-inline.

function cspFor(paths, { indexHtml, permissions, siteUrl, reportUri }) {
  const { CSPProcessingService, CSPInjectionService } = paths.require('@forge/csp');
  const cheerio = paths.require('cheerio');
  const quiet = { info() {}, warn() {}, error() {}, debug() {} };
  const processing = new CSPProcessingService(quiet, cheerio.load);
  const existing = processing.getCspDetails(indexHtml, permissions ?? {});
  const directives = new CSPInjectionService().getInjectableCSP({ existingCSPDetails: existing, microsEnv: 'prod', hostname: siteUrl });
  return directives.map((d) => (d.startsWith('report-uri ') ? `report-uri ${reportUri}` : d)).join('; ');
}

module.exports = { cspFor };
