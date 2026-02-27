import type { S3ProviderCapabilities } from '$lib/types';

import awsIcon from '$lib/assets/providers/aws.svg?url';
import minioIcon from '$lib/assets/providers/minio.svg?url';
import b2Icon from '$lib/assets/providers/b2.svg?url';
import r2Icon from '$lib/assets/providers/r2.svg?url';
import doIcon from '$lib/assets/providers/do.svg?url';
import linodeIcon from '$lib/assets/providers/linode.svg?url';
import wasabiIcon from '$lib/assets/providers/wasabi.svg?url';
import gcsIcon from '$lib/assets/providers/gcs.svg?url';
import genericIcon from '$lib/assets/providers/generic.svg?url';

// Auto-discover PNG icons for imported providers (Vite glob import)
const pngIconModules = import.meta.glob<string>('../assets/providers/*.png', { eager: true, query: '?url', import: 'default' });
const pngIcons: Record<string, string> = {};
for (const [path, url] of Object.entries(pngIconModules)) {
  const id = path.split('/').pop()!.replace('.png', '');
  pngIcons[id] = url;
}

// S3 provider data includes entries derived from Cyberduck connection profiles
// https://github.com/iterate-ch/profiles
// Copyright (c) iterate GmbH. Licensed under the GNU General Public License.
import cyberduckData from './cyberduck-s3-providers.json';

export interface S3ProviderRegion {
  id: string;        // region code: "ca-central-1"
  name: string;      // display name: "Canada (Toronto)"
  endpoint: string;  // region-specific endpoint (empty = endpoint doesn't change per-region)
}

export interface S3ProviderProfile {
  id: string;
  name: string;
  icon: string;
  endpointHint: string;
  regionHint: string;
  regions?: S3ProviderRegion[];
  website?: string;
  capabilities: S3ProviderCapabilities;
}

const ALL_STORAGE_CLASSES = [
  'STANDARD', 'STANDARD_IA', 'ONEZONE_IA', 'INTELLIGENT_TIERING',
  'GLACIER', 'DEEP_ARCHIVE', 'GLACIER_IR',
];

const ALL_TRUE: S3ProviderCapabilities = {
  versioning: true,
  lifecycleRules: true,
  cors: true,
  bucketPolicy: true,
  acl: true,
  publicAccessBlock: true,
  encryption: true,
  storageClasses: ALL_STORAGE_CLASSES,
  glacierRestore: true,
  presignedUrls: true,
  objectMetadata: true,
  objectTags: true,
  bucketTags: true,
  multipartUploadCleanup: true,
  websiteHosting: true,
  requesterPays: true,
  objectOwnership: true,
  serverAccessLogging: true,
  objectLock: true,
  listBuckets: true,
  cloudfront: true,
  inventory: true,
};

// ── Curated providers (manually maintained, with specific capabilities) ────

const CURATED_PROVIDERS: S3ProviderProfile[] = [
  {
    id: 'aws',
    name: 'Amazon S3',
    icon: awsIcon,
    endpointHint: '',
    regionHint: 'us-east-1',
    capabilities: { ...ALL_TRUE },
  },
  {
    id: 'minio',
    name: 'MinIO',
    icon: minioIcon,
    endpointHint: 'http://localhost:9000',
    regionHint: 'us-east-1',
    capabilities: {
      versioning: true,
      lifecycleRules: true,
      cors: false,
      bucketPolicy: true,
      acl: false,
      publicAccessBlock: false,
      encryption: true,
      storageClasses: ['STANDARD'],
      glacierRestore: false,
      presignedUrls: true,
      objectMetadata: true,
      objectTags: true,
      bucketTags: true,
      multipartUploadCleanup: true,
      websiteHosting: false,
      requesterPays: false,
      objectOwnership: false,
      serverAccessLogging: false,
      objectLock: true,
      listBuckets: true,
      cloudfront: false,
      inventory: false,
    },
  },
  {
    id: 'b2',
    name: 'Backblaze B2',
    icon: b2Icon,
    endpointHint: 'https://s3.us-west-004.backblazeb2.com',
    regionHint: 'us-west-004',
    capabilities: {
      versioning: false,
      lifecycleRules: true,
      cors: true,
      bucketPolicy: false,
      acl: false,
      publicAccessBlock: false,
      encryption: false,
      storageClasses: ['STANDARD'],
      glacierRestore: false,
      presignedUrls: true,
      objectMetadata: true,
      objectTags: false,
      bucketTags: false,
      multipartUploadCleanup: true,
      websiteHosting: false,
      requesterPays: false,
      objectOwnership: false,
      serverAccessLogging: false,
      objectLock: false,
      listBuckets: true,
      cloudfront: false,
      inventory: false,
    },
  },
  {
    id: 'r2',
    name: 'Cloudflare R2',
    icon: r2Icon,
    endpointHint: 'https://<account-id>.r2.cloudflarestorage.com',
    regionHint: 'auto',
    capabilities: {
      versioning: false,
      lifecycleRules: false,
      cors: false,
      bucketPolicy: false,
      acl: false,
      publicAccessBlock: false,
      encryption: false,
      storageClasses: ['STANDARD'],
      glacierRestore: false,
      presignedUrls: true,
      objectMetadata: true,
      objectTags: false,
      bucketTags: false,
      multipartUploadCleanup: true,
      websiteHosting: false,
      requesterPays: false,
      objectOwnership: false,
      serverAccessLogging: false,
      objectLock: false,
      listBuckets: true,
      cloudfront: false,
      inventory: false,
    },
  },
  {
    id: 'do',
    name: 'DigitalOcean Spaces',
    icon: doIcon,
    endpointHint: 'https://nyc3.digitaloceanspaces.com',
    regionHint: 'nyc3',
    capabilities: {
      versioning: false,
      lifecycleRules: true,
      cors: true,
      bucketPolicy: false,
      acl: true,
      publicAccessBlock: false,
      encryption: false,
      storageClasses: ['STANDARD'],
      glacierRestore: false,
      presignedUrls: true,
      objectMetadata: true,
      objectTags: false,
      bucketTags: false,
      multipartUploadCleanup: true,
      websiteHosting: false,
      requesterPays: false,
      objectOwnership: false,
      serverAccessLogging: false,
      objectLock: false,
      listBuckets: true,
      cloudfront: false,
      inventory: false,
    },
  },
  {
    id: 'linode',
    name: 'Linode Object Storage',
    icon: linodeIcon,
    endpointHint: 'https://us-east-1.linodeobjects.com',
    regionHint: 'us-east-1',
    capabilities: {
      versioning: true,
      lifecycleRules: true,
      cors: true,
      bucketPolicy: false,
      acl: true,
      publicAccessBlock: false,
      encryption: false,
      storageClasses: ['STANDARD'],
      glacierRestore: false,
      presignedUrls: true,
      objectMetadata: true,
      objectTags: false,
      bucketTags: false,
      multipartUploadCleanup: true,
      websiteHosting: false,
      requesterPays: false,
      objectOwnership: false,
      serverAccessLogging: false,
      objectLock: false,
      listBuckets: false,
      cloudfront: false,
      inventory: false,
    },
  },
  {
    id: 'wasabi',
    name: 'Wasabi',
    icon: wasabiIcon,
    endpointHint: 'https://s3.wasabisys.com',
    regionHint: 'us-east-1',
    capabilities: {
      versioning: true,
      lifecycleRules: false,
      cors: false,
      bucketPolicy: true,
      acl: true,
      publicAccessBlock: false,
      encryption: false,
      storageClasses: ['STANDARD'],
      glacierRestore: false,
      presignedUrls: true,
      objectMetadata: true,
      objectTags: true,
      bucketTags: true,
      multipartUploadCleanup: true,
      websiteHosting: false,
      requesterPays: false,
      objectOwnership: false,
      serverAccessLogging: false,
      objectLock: true,
      listBuckets: true,
      cloudfront: false,
      inventory: false,
    },
  },
  {
    id: 'gcs',
    name: 'Google Cloud Storage',
    icon: gcsIcon,
    endpointHint: 'https://storage.googleapis.com',
    regionHint: 'us-east1',
    capabilities: {
      versioning: true,
      lifecycleRules: true,
      cors: true,
      bucketPolicy: false,
      acl: true,
      publicAccessBlock: false,
      encryption: true,
      storageClasses: ['STANDARD', 'NEARLINE', 'COLDLINE', 'ARCHIVE'],
      glacierRestore: false,
      presignedUrls: true,
      objectMetadata: true,
      objectTags: false,
      bucketTags: true,
      multipartUploadCleanup: true,
      websiteHosting: false,
      requesterPays: false,
      objectOwnership: false,
      serverAccessLogging: false,
      objectLock: false,
      listBuckets: true,
      cloudfront: false,
      inventory: false,
    },
  },
];

// ── Merge Cyberduck-imported providers ─────────────────────────────────────

// Map curated provider ids to their Cyberduck counterpart ids for merging regions
const CYBERDUCK_ID_MAP: Record<string, string> = {
  b2: 'backblaze',
  do: 'digitalocean',
};

function buildProviderList(): S3ProviderProfile[] {
  const cyberduckProviders = cyberduckData.providers as Array<{
    id: string;
    name: string;
    endpointHint: string;
    regionHint: string;
    regions?: S3ProviderRegion[];
    website?: string;
  }>;

  // Build a map of Cyberduck providers by id
  const cyberduckMap = new Map(cyberduckProviders.map(p => [p.id, p]));

  // Start with curated providers, enriching with Cyberduck region data
  const result: S3ProviderProfile[] = CURATED_PROVIDERS.map(curated => {
    const cyberduckId = CYBERDUCK_ID_MAP[curated.id] ?? curated.id;
    const cyberduck = cyberduckMap.get(cyberduckId);
    if (cyberduck?.regions) {
      cyberduckMap.delete(cyberduckId); // consumed — don't duplicate
      return { ...curated, regions: cyberduck.regions, website: cyberduck.website };
    }
    if (cyberduck) {
      cyberduckMap.delete(cyberduckId);
    }
    return curated;
  });

  // Add remaining Cyberduck providers that aren't already curated
  for (const [, cp] of cyberduckMap) {
    result.push({
      id: cp.id,
      name: cp.name,
      icon: pngIcons[cp.id] ?? genericIcon,
      endpointHint: cp.endpointHint,
      regionHint: cp.regionHint,
      ...(cp.regions ? { regions: cp.regions } : {}),
      ...(cp.website ? { website: cp.website } : {}),
      capabilities: { ...ALL_TRUE },
    });
  }

  // Always have Custom / Other at the end
  result.push({
    id: 'custom',
    name: 'Custom / Other',
    icon: genericIcon,
    endpointHint: 'https://...',
    regionHint: 'us-east-1',
    capabilities: { ...ALL_TRUE },
  });

  return result;
}

export const S3_PROVIDERS: S3ProviderProfile[] = buildProviderList();

const providerMap = new Map(S3_PROVIDERS.map(p => [p.id, p]));

export function getProvider(id: string): S3ProviderProfile {
  return providerMap.get(id) ?? providerMap.get('custom')!;
}

export function resolveCapabilities(profile: { provider?: string; customCapabilities?: S3ProviderCapabilities }): S3ProviderCapabilities {
  if (profile.customCapabilities) {
    return profile.customCapabilities;
  }
  return getProvider(profile.provider ?? 'aws').capabilities;
}

const endpointPatterns: [RegExp, string][] = [
  // Curated providers
  [/\.backblazeb2\.com/i, 'b2'],
  [/\.r2\.cloudflarestorage\.com/i, 'r2'],
  [/\.digitaloceanspaces\.com/i, 'do'],
  [/\.linodeobjects\.com/i, 'linode'],
  [/\.wasabisys\.com/i, 'wasabi'],
  [/storage\.googleapis\.com/i, 'gcs'],
  [/localhost|127\.0\.0\.1|\.local[:/]/, 'minio'],
  // Cyberduck-imported providers
  [/\.aliyuncs\.com/i, 'alibaba'],
  [/\.byteark\.com/i, 'byteark'],
  [/\.contabostorage\.com/i, 'contabo'],
  [/cwobject\.com/i, 'coreweave'],
  [/\.dinaserver\.com/i, 'dinahosting'],
  [/\.dream\.io/i, 'dreamhost'],
  [/\.dunkel\.de/i, 'dunkel'],
  [/\.fastlystorage\.app/i, 'fastly'],
  [/\.filebase\.com/i, 'filebase'],
  [/\.fornex\.io/i, 'fornex'],
  [/\.your-objectstorage\.com/i, 'hetzner'],
  [/cloud-object-storage\.appdomain\.cloud/i, 'ibm-cos'],
  [/\.impossibleapi\.net/i, 'impossiblecloud'],
  [/\.infomaniak\.(cloud|com)/i, 'infomaniak'],
  [/\.ionoscloud\.com/i, 'ionos'],
  [/\.katapultobjects\.com/i, 'katapult'],
  [/\.s4\.mega\.io/i, 'mega'],
  [/\.myqnapcloud\.io/i, 'myqnapcloud'],
  [/\.io\.cloud\.ovh\.net/i, 'ovh'],
  [/\.pilw\.io/i, 'pilvio'],
  [/\.psychz\.net/i, 'psychz'],
  [/\.scw\.cloud/i, 'scaleway'],
  [/\.storadera\.com/i, 'storadera'],
  [/\.storjshare\.io/i, 'storj'],
  [/\.swisscom\.com/i, 'swisscom'],
  [/\.synologyc2\.net/i, 'synology'],
  [/\.tigris\.dev/i, 'tigris'],
  [/\.vitanium\.com/i, 'vitanium'],
  [/\.z1storage\.com/i, 'z1'],
  [/\.zeroservices\.eu/i, 'zero'],
];

export function inferProviderFromEndpoint(endpoint?: string): string {
  if (!endpoint) return 'aws';
  for (const [pattern, id] of endpointPatterns) {
    if (pattern.test(endpoint)) return id;
  }
  return 'custom';
}

const iconMap: Record<string, string> = {
  aws: awsIcon,
  minio: minioIcon,
  b2: b2Icon,
  r2: r2Icon,
  do: doIcon,
  linode: linodeIcon,
  wasabi: wasabiIcon,
  gcs: gcsIcon,
  custom: genericIcon,
  ...pngIcons,
};

export function getProviderIcon(id: string): string {
  return iconMap[id] ?? genericIcon;
}
