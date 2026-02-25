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

export interface S3ProviderProfile {
  id: string;
  name: string;
  icon: string;
  endpointHint: string;
  regionHint: string;
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
};

export const S3_PROVIDERS: S3ProviderProfile[] = [
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
    },
  },
  {
    id: 'custom',
    name: 'Custom / Other',
    icon: genericIcon,
    endpointHint: 'https://...',
    regionHint: 'us-east-1',
    capabilities: { ...ALL_TRUE },
  },
];

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
  [/\.backblazeb2\.com/i, 'b2'],
  [/\.r2\.cloudflarestorage\.com/i, 'r2'],
  [/\.digitaloceanspaces\.com/i, 'do'],
  [/\.linodeobjects\.com/i, 'linode'],
  [/\.wasabisys\.com/i, 'wasabi'],
  [/storage\.googleapis\.com/i, 'gcs'],
  [/localhost|127\.0\.0\.1|\.local[:/]/, 'minio'],
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
};

export function getProviderIcon(id: string): string {
  return iconMap[id] ?? genericIcon;
}
