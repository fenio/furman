#!/usr/bin/env node
/**
 * Import S3 provider profiles from Cyberduck's community-maintained profiles repo.
 *
 * Usage:
 *   node scripts/import-cyberduck-profiles.mjs [path-to-local-profiles-repo]
 *
 * If no local path is given, clones iterate-ch/profiles into a temp directory.
 * Outputs: src/lib/data/cyberduck-s3-providers.json
 */

import { execSync } from 'node:child_process';
import { mkdtempSync, readdirSync, rmSync } from 'node:fs';
import { writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';

const OUT_PATH = resolve(import.meta.dirname, '..', 'src', 'lib', 'data', 'cyberduck-s3-providers.json');

// ── Hostname → family id mapping ──────────────────────────────────────────
// Primary grouping strategy: group profiles by their hostname domain pattern.
const HOSTNAME_FAMILIES = [
  [/\.aliyuncs\.com$/i,                 'alibaba',      'Alibaba Cloud OSS'],
  [/\.backblazeb2\.com$/i,              'backblaze',    'Backblaze B2'],
  [/\.byteark\.com$/i,                  'byteark',      'ByteArk Storage'],
  [/\.contabostorage\.com$/i,           'contabo',      'Contabo Object Storage'],
  [/(^|\.)cwobject\.com$/i,              'coreweave',    'CoreWeave Object Storage'],
  [/\.digitaloceanspaces\.com$/i,       'digitalocean', 'DigitalOcean Spaces'],
  [/\.dinaserver\.com$/i,               'dinahosting',  'Dinahosting S3'],
  [/\.dream\.io$/i,                     'dreamhost',    'DreamObjects'],
  [/\.dunkel\.de$/i,                    'dunkel',       'Dunkel Cloud Storage'],
  [/\.fastlystorage\.app$/i,            'fastly',       'Fastly Object Storage'],
  [/\.filebase\.com$/i,                 'filebase',     'Filebase'],
  [/\.fornex\.io$/i,                    'fornex',       'Fornex Cold Storage'],
  [/\.your-objectstorage\.com$/i,       'hetzner',      'Hetzner Object Storage'],
  [/cloud-object-storage\.appdomain\.cloud$/i, 'ibm-cos', 'IBM Cloud Object Storage'],
  [/\.impossibleapi\.net$/i,            'impossiblecloud', 'Impossible Cloud'],
  [/\.infomaniak\.cloud$/i,             'infomaniak',   'Infomaniak Public Cloud'],
  [/\.infomaniak\.com$/i,              'infomaniak',   'Infomaniak Public Cloud'],
  [/\.ionoscloud\.com$/i,               'ionos',        'IONOS Cloud Object Storage'],
  [/\.katapultobjects\.com$/i,          'katapult',     'Katapult Object Storage'],
  [/\.linodeobjects\.com$/i,            'linode',       'Linode Object Storage'],
  [/\.s4\.mega\.io$/i,                  'mega',         'MEGA S4'],
  [/\.myqnapcloud\.io$/i,              'myqnapcloud',  'myQNAPcloud Object Storage'],
  [/\.nipa\.cloud$/i,                   'nipa',         'Nipa Cloud Space'],
  [/\.io\.cloud\.ovh\.net$/i,           'ovh',          'OVHcloud Object Storage'],
  [/\.pilw\.io$/i,                      'pilvio',       'Pilvio S3'],
  [/\.psychz\.net$/i,                   'psychz',       'Psychz sObject'],
  [/\.scw\.cloud$/i,                    'scaleway',     'Scaleway Object Storage'],
  [/\.storadera\.com$/i,                'storadera',    'Storadera'],
  [/\.storjshare\.io$/i,                'storj',        'Storj DCS'],
  [/\.swisscom\.com$/i,                 'swisscom',     'Swisscom S3 Dynamic Storage'],
  [/\.synologyc2\.net$/i,               'synology',     'Synology C2 Object Storage'],
  [/\.tigris\.dev$/i,                   'tigris',       'Tigris Object Storage'],
  [/\.vitanium\.com$/i,                 'vitanium',     'Vitanium Object Storage'],
  [/\.wasabisys\.com$/i,                'wasabi',       'Wasabi'],
  [/\.z1storage\.com$/i,                'z1',           'Z1 Storage'],
  [/\.zeroservices\.eu$/i,              'zero',         'ZERO-Z3'],
  [/\.amazonaws\.com\.cn$/i,            's3-china',     'AWS China'],
];

// Vendor patterns to skip entirely (we have our own curated entries, or they're meta/utility profiles)
const SKIP_VENDORS = new Set([
  's3',               // generic AWS
  's3-gov',           // AWS GovCloud
  's3-glacier',       // Glacier variant
  'gcs',              // Google Cloud Storage
  'minio',            // MinIO
  's3-cloudflare-r2', // Cloudflare R2 — we have our own
]);

const SKIP_VENDOR_PREFIXES = [
  's3-',              // s3-cli, s3-role, s3-path-style, s3-http, s3-timestamps, etc.
  'aws-s3-',          // aws-s3-sts-*, aws-s3-getsessiontoken
  's3-aws2-',         // legacy sig v2 profiles
];

// Vendor → family override for profiles that don't have a hostname
const VENDOR_OVERRIDE = {
  'garage':    { family: 'garage',    name: 'Garage S3' },
  'upcloud':   { family: 'upcloud',   name: 'UpCloud Object Storage' },
  'qumulo-s3': { family: 'qumulo',    name: 'Qumulo S3' },
};

// ── Helpers ────────────────────────────────────────────────────────────────

function parseProfile(filePath) {
  try {
    const json = execSync(`plutil -convert json -o - "${filePath}"`, {
      encoding: 'utf8',
      timeout: 5000,
    });
    return JSON.parse(json);
  } catch {
    return null;
  }
}

function findProfiles(dir) {
  const results = [];
  for (const entry of readdirSync(dir, { withFileTypes: true, recursive: true })) {
    if (entry.name.endsWith('.cyberduckprofile')) {
      results.push(join(entry.parentPath ?? entry.path, entry.name));
    }
  }
  return results;
}

/**
 * Determine family id and display name from a profile's hostname and vendor.
 * Returns { family, name } or null to skip.
 */
function classifyProfile(profile) {
  const vendor = (profile.Vendor || '').toLowerCase();
  const hostname = profile['Default Hostname'] || '';

  // Skip known vendors
  if (SKIP_VENDORS.has(vendor)) return null;
  for (const prefix of SKIP_VENDOR_PREFIXES) {
    if (vendor.startsWith(prefix)) return null;
  }
  // Skip AWS PrivateLink
  if (vendor.includes('privatelink')) return null;

  // Try hostname-based classification first
  for (const [pattern, family, name] of HOSTNAME_FAMILIES) {
    if (pattern.test(hostname)) {
      return { family, name };
    }
  }

  // Try vendor override
  if (VENDOR_OVERRIDE[vendor]) {
    return VENDOR_OVERRIDE[vendor];
  }

  // Oracle OCI uses dynamic hostnames based on namespace — group by vendor prefix
  if (vendor.startsWith('oracle')) {
    return { family: 'oracle', name: 'Oracle Cloud Infrastructure Object Storage' };
  }

  // Skip profiles with no hostname and no override (generic/utility profiles)
  if (!hostname) return null;

  // Fallback: use vendor as family id
  return { family: vendor, name: extractBaseName(profile.Description) || vendor };
}

/**
 * Extract a user-friendly display name from the Description field.
 * e.g. "Wasabi (Toronto)" → "Wasabi"
 * e.g. "OVHcloud Object Storage (Frankfurt)" → "OVHcloud Object Storage"
 */
function extractBaseName(description) {
  if (!description) return null;
  return description.replace(/\s*\([^)]+\)\s*$/, '').trim();
}

/**
 * Extract region info from Description's parenthetical.
 * e.g. "Wasabi (Toronto)" → "Toronto"
 * e.g. "Linode Object Storage (Milan, IT) (it-mil-1)" → "Milan, IT"
 */
function extractRegionName(description) {
  if (!description) return null;
  // Match last parenthetical (some have two)
  const parens = [...description.matchAll(/\(([^)]+)\)/g)];
  if (parens.length === 0) return null;
  // If there are two parentheticals, use the first (human-readable) one
  // e.g. "Linode Object Storage (Milan, IT) (it-mil-1)"
  if (parens.length >= 2) return parens[0][1].trim();
  return parens[0][1].trim();
}

// ── Main ───────────────────────────────────────────────────────────────────

function main() {
  let repoDir = process.argv[2];
  let tempDir = null;

  if (!repoDir) {
    tempDir = mkdtempSync(join(tmpdir(), 'cyberduck-profiles-'));
    console.log(`Cloning iterate-ch/profiles into ${tempDir}...`);
    execSync(`git clone --depth 1 https://github.com/iterate-ch/profiles.git "${tempDir}"`, {
      stdio: 'inherit',
      timeout: 60000,
    });
    repoDir = tempDir;
  } else {
    repoDir = resolve(repoDir);
    console.log(`Using local profiles repo: ${repoDir}`);
  }

  try {
    const profileFiles = findProfiles(repoDir);
    console.log(`Found ${profileFiles.length} .cyberduckprofile files`);

    // Parse all profiles and filter for S3
    const s3Profiles = [];
    let skippedCount = 0;
    for (const f of profileFiles) {
      const p = parseProfile(f);
      if (!p) continue;
      if (p.Protocol !== 's3') continue;
      s3Profiles.push(p);
    }
    console.log(`${s3Profiles.length} are S3-compatible`);

    // Group by family
    const families = new Map(); // family → { name, profiles: [...] }
    for (const p of s3Profiles) {
      const classification = classifyProfile(p);
      if (!classification) {
        skippedCount++;
        continue;
      }

      const { family, name } = classification;
      if (!families.has(family)) {
        families.set(family, { name, profiles: [] });
      }
      families.get(family).profiles.push(p);
    }

    console.log(`Skipped ${skippedCount} profiles (AWS/GCS/MinIO/R2/utility)`);
    console.log(`Grouped into ${families.size} provider families`);

    // Build output
    const providers = [];
    for (const [familyId, data] of [...families.entries()].sort((a, b) => a[1].name.localeCompare(b[1].name))) {
      const regions = [];
      let defaultEndpoint = '';
      let defaultRegion = '';
      let website = '';

      for (const p of data.profiles) {
        const hostname = p['Default Hostname'] || '';
        const port = p['Default Port'];
        const regionName = extractRegionName(p.Description);
        const regionCode = p.Region || '';

        // Build the endpoint URL
        let endpointUrl = '';
        if (hostname) {
          const scheme = (p.Scheme ?? 'https').toLowerCase();
          endpointUrl = port ? `${scheme}://${hostname}:${port}` : `${scheme}://${hostname}`;
        }

        // Collect website if present
        if (p['Help Url'] && !website) {
          website = p['Help Url'];
        }

        // Build region entry
        regions.push({
          id: regionCode || regionName?.toLowerCase().replace(/[^a-z0-9]+/g, '-') || '',
          name: regionName || regionCode || hostname,
          endpoint: endpointUrl,
        });

        // Use first profile as the default
        if (!defaultEndpoint && endpointUrl) {
          defaultEndpoint = endpointUrl;
          defaultRegion = regionCode || 'us-east-1';
        }
        if (!defaultRegion && regionCode) {
          defaultRegion = regionCode;
        }
      }

      // Deduplicate regions by endpoint
      const seenEndpoints = new Set();
      const uniqueRegions = [];
      for (const r of regions) {
        const key = r.endpoint || r.id;
        if (!seenEndpoints.has(key)) {
          seenEndpoints.add(key);
          uniqueRegions.push(r);
        }
      }

      const entry = {
        id: familyId,
        name: data.name,
        endpointHint: defaultEndpoint,
        regionHint: defaultRegion || 'us-east-1',
        ...(uniqueRegions.length > 1 ? { regions: uniqueRegions } : {}),
        ...(website ? { website } : {}),
      };

      providers.push(entry);
    }

    const output = {
      _attribution: 'Provider data derived from Cyberduck connection profiles (https://github.com/iterate-ch/profiles). Copyright iterate GmbH. Licensed GPL-3.0.',
      _generated: new Date().toISOString().slice(0, 10),
      providers,
    };

    writeFileSync(OUT_PATH, JSON.stringify(output, null, 2) + '\n');
    console.log(`\nWrote ${providers.length} providers to ${OUT_PATH}`);

    // Summary
    const withRegions = providers.filter(p => p.regions);
    console.log(`  ${withRegions.length} providers with regional variants`);
    console.log(`  ${providers.length - withRegions.length} single-endpoint providers`);
    const totalRegions = withRegions.reduce((sum, p) => sum + p.regions.length, 0);
    console.log(`  ${totalRegions} total regional endpoints`);
  } finally {
    if (tempDir) {
      console.log(`\nCleaning up temp dir...`);
      rmSync(tempDir, { recursive: true, force: true });
    }
  }
}

main();
