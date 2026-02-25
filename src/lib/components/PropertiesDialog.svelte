<script lang="ts">
  import { onMount } from 'svelte';
  import { appState } from '$lib/state/app.svelte';
  import { getFileProperties, getDirectorySize } from '$lib/services/tauri';
  import {
    s3HeadObject, s3ChangeStorageClass, s3RestoreObject, s3ListObjectVersions,
    s3DownloadVersion, s3RestoreVersion, s3DeleteVersion,
    s3GetBucketVersioning, s3PutBucketVersioning, s3GetBucketEncryption,
    s3GetBucketTags, s3PutBucketTags, s3ListMultipartUploads, s3AbortMultipartUpload,
    s3GetObjectMetadata, s3PutObjectMetadata, s3GetObjectTags, s3PutObjectTags,
    s3GetBucketLifecycle, s3PutBucketLifecycle,
    s3GetBucketCors, s3PutBucketCors,
    s3GetPublicAccessBlock, s3PutPublicAccessBlock,
    s3GetBucketPolicy, s3PutBucketPolicy,
    s3GetBucketAcl,
  } from '$lib/services/s3';
  import { invoke } from '@tauri-apps/api/core';
  import { formatSize, formatDate, formatPermissions } from '$lib/utils/format';
  import type {
    FileProperties, S3ObjectProperties, S3ObjectVersion, PanelBackend,
    S3BucketVersioning, S3BucketEncryption, S3Tag, S3MultipartUpload,
    S3ObjectMetadata, S3LifecycleRule, S3CorsRule, S3PublicAccessBlock, S3BucketAcl,
  } from '$lib/types';

  interface Props {
    path: string;
    backend: PanelBackend;
    s3ConnectionId: string;
    onClose: () => void;
  }

  let { path, backend, s3ConnectionId, onClose }: Props = $props();

  let fileProps = $state<FileProperties | null>(null);
  let s3Props = $state<S3ObjectProperties | null>(null);
  let s3IsPrefix = $state(false);
  let s3IsBucketRoot = $state(false);
  let loading = $state(true);
  let error = $state('');
  let dirSize = $state<number | null>(null);
  let dirSizeLoading = $state(false);

  // Editable permissions state
  let editMode = $state(0);
  let permsDirty = $state(false);
  let applyingPerms = $state(false);

  const permBits = [
    { label: 'r', bit: 0o400, row: 'Owner' },
    { label: 'w', bit: 0o200, row: 'Owner' },
    { label: 'x', bit: 0o100, row: 'Owner' },
    { label: 'r', bit: 0o040, row: 'Group' },
    { label: 'w', bit: 0o020, row: 'Group' },
    { label: 'x', bit: 0o010, row: 'Group' },
    { label: 'r', bit: 0o004, row: 'Other' },
    { label: 'w', bit: 0o002, row: 'Other' },
    { label: 'x', bit: 0o001, row: 'Other' },
  ];

  function toggleBit(bit: number) {
    editMode = editMode ^ bit;
    permsDirty = true;
  }

  function hasBit(bit: number): boolean {
    return (editMode & bit) !== 0;
  }

  function octalString(): string {
    return '0' + ((editMode >> 6) & 7).toString() + ((editMode >> 3) & 7).toString() + (editMode & 7).toString();
  }

  function handleOctalInput(e: Event) {
    const val = (e.target as HTMLInputElement).value;
    const parsed = parseInt(val, 8);
    if (!isNaN(parsed) && parsed >= 0 && parsed <= 0o777) {
      editMode = parsed;
      permsDirty = true;
    }
  }

  async function applyPermissions() {
    if (!fileProps) return;
    applyingPerms = true;
    try {
      await invoke('set_permissions', { path: fileProps.path, mode: editMode });
      fileProps.permissions = editMode;
      permsDirty = false;
    } catch (err: unknown) {
      error = String(err);
    } finally {
      applyingPerms = false;
    }
  }

  // Storage class management
  const storageClasses = [
    'STANDARD', 'STANDARD_IA', 'ONEZONE_IA', 'INTELLIGENT_TIERING',
    'GLACIER', 'DEEP_ARCHIVE', 'GLACIER_IR',
  ];
  let selectedStorageClass = $state('');
  let applyingClass = $state(false);
  let classMessage = $state('');

  const isGlacier = $derived(
    s3Props?.storage_class === 'GLACIER' ||
    s3Props?.storage_class === 'DEEP_ARCHIVE' ||
    s3Props?.storage_class === 'GLACIER_IR'
  );

  // Glacier restore
  let restoreDays = $state(7);
  let restoreTier = $state('Standard');
  let restoringGlacier = $state(false);
  let restoreMessage = $state('');

  // Versioning (object-level)
  let versionsExpanded = $state(false);
  let versions = $state<S3ObjectVersion[]>([]);
  let versionsLoading = $state(false);
  let versionsError = $state('');
  let versionActionLoading = $state<string | null>(null);

  // Bucket-level: Versioning
  let bucketVersioning = $state<S3BucketVersioning | null>(null);
  let bucketVersioningLoading = $state(false);
  let applyingVersioning = $state(false);
  let versioningMessage = $state('');

  // Bucket-level: Encryption
  let bucketEncryption = $state<S3BucketEncryption | null>(null);
  let bucketEncryptionLoading = $state(false);

  // Bucket-level: Tags
  let bucketTagsExpanded = $state(false);
  let bucketTags = $state<S3Tag[]>([]);
  let bucketTagsOriginal = $state<string>('');
  let bucketTagsLoading = $state(false);
  let bucketTagsMessage = $state('');
  let savingBucketTags = $state(false);

  // Bucket-level: Incomplete uploads
  let uploadsExpanded = $state(false);
  let multipartUploads = $state<S3MultipartUpload[]>([]);
  let uploadsLoading = $state(false);
  let uploadsError = $state('');
  let abortingUpload = $state<string | null>(null);
  let abortingAll = $state(false);

  // Bucket-level: Lifecycle Rules
  let lifecycleExpanded = $state(false);
  let lifecycleRules = $state<S3LifecycleRule[]>([]);
  let lifecycleOriginal = $state('');
  let lifecycleLoading = $state(false);
  let lifecycleMessage = $state('');
  let savingLifecycle = $state(false);
  let editingRuleIndex = $state<number | null>(null);

  const lifecycleDirty = $derived(JSON.stringify(lifecycleRules) !== lifecycleOriginal);

  const lifecycleStorageClasses = [
    'STANDARD_IA', 'ONEZONE_IA', 'INTELLIGENT_TIERING',
    'GLACIER', 'GLACIER_IR', 'DEEP_ARCHIVE',
  ];

  // Bucket-level: CORS Configuration
  let corsExpanded = $state(false);
  let corsRules = $state<S3CorsRule[]>([]);
  let corsOriginal = $state('');
  let corsLoading = $state(false);
  let corsMessage = $state('');
  let savingCors = $state(false);

  const corsDirty = $derived(JSON.stringify(corsRules) !== corsOriginal);

  const corsMethods = ['GET', 'PUT', 'POST', 'DELETE', 'HEAD'];

  // Bucket-level: Public Access Block
  let publicAccessBlock = $state<S3PublicAccessBlock | null>(null);
  let publicAccessLoading = $state(false);
  let publicAccessMessage = $state('');
  let savingPublicAccess = $state(false);
  let pabBlockPublicAcls = $state(false);
  let pabIgnorePublicAcls = $state(false);
  let pabBlockPublicPolicy = $state(false);
  let pabRestrictPublicBuckets = $state(false);
  let pabOriginal = $state('');

  const publicAccessDirty = $derived(
    JSON.stringify({ a: pabBlockPublicAcls, b: pabIgnorePublicAcls, c: pabBlockPublicPolicy, d: pabRestrictPublicBuckets }) !== pabOriginal
  );

  // Bucket-level: Bucket Policy
  let policyExpanded = $state(false);
  let policyText = $state('');
  let policyOriginal = $state('');
  let policyLoading = $state(false);
  let policyMessage = $state('');
  let savingPolicy = $state(false);
  let policyJsonValid = $state(true);

  const policyDirty = $derived(policyText !== policyOriginal);

  // Bucket-level: ACL (read-only)
  let aclExpanded = $state(false);
  let bucketAcl = $state<S3BucketAcl | null>(null);
  let aclLoading = $state(false);
  let aclError = $state('');

  // Object-level: Metadata
  let metadataExpanded = $state(false);
  let objectMetadata = $state<S3ObjectMetadata | null>(null);
  let metadataLoading = $state(false);
  let metadataMessage = $state('');
  let savingMetadata = $state(false);
  let metaContentType = $state('');
  let metaContentDisposition = $state('');
  let metaCacheControl = $state('');
  let metaContentEncoding = $state('');
  let metaCustom = $state<{key: string; value: string}[]>([]);
  let metaOriginal = $state('');

  // Object-level: Tags
  let objTagsExpanded = $state(false);
  let objectTags = $state<S3Tag[]>([]);
  let objTagsOriginal = $state<string>('');
  let objTagsLoading = $state(false);
  let objTagsMessage = $state('');
  let savingObjTags = $state(false);

  const bucketTagsDirty = $derived(JSON.stringify(bucketTags) !== bucketTagsOriginal);
  const objTagsDirty = $derived(JSON.stringify(objectTags) !== objTagsOriginal);
  const metadataDirty = $derived(
    JSON.stringify({ ct: metaContentType, cd: metaContentDisposition, cc: metaCacheControl, ce: metaContentEncoding, custom: metaCustom }) !== metaOriginal
  );

  async function applyStorageClass() {
    if (!s3Props || !selectedStorageClass || selectedStorageClass === s3Props.storage_class) return;
    applyingClass = true;
    classMessage = '';
    try {
      await s3ChangeStorageClass(s3ConnectionId, path, selectedStorageClass);
      s3Props.storage_class = selectedStorageClass;
      classMessage = 'Storage class updated';
    } catch (err: unknown) {
      classMessage = 'Error: ' + String(err);
    } finally {
      applyingClass = false;
    }
  }

  async function restoreFromGlacier() {
    if (!s3Props) return;
    restoringGlacier = true;
    restoreMessage = '';
    try {
      await s3RestoreObject(s3ConnectionId, path, restoreDays, restoreTier);
      restoreMessage = 'Restore initiated';
      s3Props = await s3HeadObject(s3ConnectionId, path);
    } catch (err: unknown) {
      restoreMessage = 'Error: ' + String(err);
    } finally {
      restoringGlacier = false;
    }
  }

  async function loadVersions() {
    if (versionsLoading) return;
    versionsExpanded = !versionsExpanded;
    if (!versionsExpanded || versions.length > 0) return;
    versionsLoading = true;
    versionsError = '';
    try {
      versions = await s3ListObjectVersions(s3ConnectionId, path);
    } catch (err: unknown) {
      versionsError = String(err);
    } finally {
      versionsLoading = false;
    }
  }

  async function handleDownloadVersion(vid: string) {
    versionActionLoading = vid;
    try {
      const tempPath = await s3DownloadVersion(s3ConnectionId, path, vid);
      const { appState: app } = await import('$lib/state/app.svelte');
      app.viewerMode = 'text';
      app.viewerPath = tempPath;
      app.modal = 'viewer';
    } catch (err: unknown) {
      versionsError = String(err);
    } finally {
      versionActionLoading = null;
    }
  }

  async function handleRestoreVersion(vid: string) {
    if (!confirm(`Restore this version as current? This will overwrite the current object.`)) return;
    versionActionLoading = vid;
    try {
      await s3RestoreVersion(s3ConnectionId, path, vid);
      versions = await s3ListObjectVersions(s3ConnectionId, path);
      s3Props = await s3HeadObject(s3ConnectionId, path);
    } catch (err: unknown) {
      versionsError = String(err);
    } finally {
      versionActionLoading = null;
    }
  }

  async function handleDeleteVersion(vid: string) {
    if (!confirm(`Permanently delete this version? This cannot be undone.`)) return;
    versionActionLoading = vid;
    try {
      await s3DeleteVersion(s3ConnectionId, path, vid);
      versions = versions.filter(v => v.version_id !== vid);
    } catch (err: unknown) {
      versionsError = String(err);
    } finally {
      versionActionLoading = null;
    }
  }

  function truncateVid(vid: string): string {
    return vid.length > 16 ? vid.slice(0, 16) + '\u2026' : vid;
  }

  // ── Bucket-level functions ──────────────────────────────────────────────

  async function toggleBucketVersioning() {
    if (!bucketVersioning) return;
    const enable = bucketVersioning.status !== 'Enabled';
    applyingVersioning = true;
    versioningMessage = '';
    try {
      await s3PutBucketVersioning(s3ConnectionId, enable);
      bucketVersioning = await s3GetBucketVersioning(s3ConnectionId);
      versioningMessage = enable ? 'Versioning enabled' : 'Versioning suspended';
    } catch (err: unknown) {
      versioningMessage = 'Error: ' + String(err);
    } finally {
      applyingVersioning = false;
    }
  }

  async function loadBucketTags() {
    if (bucketTagsLoading) return;
    bucketTagsExpanded = !bucketTagsExpanded;
    if (!bucketTagsExpanded || bucketTags.length > 0) return;
    bucketTagsLoading = true;
    bucketTagsMessage = '';
    try {
      bucketTags = await s3GetBucketTags(s3ConnectionId);
      bucketTagsOriginal = JSON.stringify(bucketTags);
    } catch (err: unknown) {
      bucketTagsMessage = 'Error: ' + String(err);
    } finally {
      bucketTagsLoading = false;
    }
  }

  function addBucketTag() {
    if (bucketTags.length >= 50) return;
    bucketTags = [...bucketTags, { key: '', value: '' }];
  }

  function removeBucketTag(index: number) {
    bucketTags = bucketTags.filter((_, i) => i !== index);
  }

  async function saveBucketTags() {
    savingBucketTags = true;
    bucketTagsMessage = '';
    try {
      const filtered = bucketTags.filter(t => t.key.trim());
      await s3PutBucketTags(s3ConnectionId, filtered);
      bucketTags = filtered;
      bucketTagsOriginal = JSON.stringify(bucketTags);
      bucketTagsMessage = 'Tags saved';
    } catch (err: unknown) {
      bucketTagsMessage = 'Error: ' + String(err);
    } finally {
      savingBucketTags = false;
    }
  }

  async function loadMultipartUploads() {
    if (uploadsLoading) return;
    uploadsExpanded = !uploadsExpanded;
    if (!uploadsExpanded) return;
    uploadsLoading = true;
    uploadsError = '';
    try {
      multipartUploads = await s3ListMultipartUploads(s3ConnectionId);
    } catch (err: unknown) {
      uploadsError = String(err);
    } finally {
      uploadsLoading = false;
    }
  }

  async function abortUpload(key: string, uploadId: string) {
    abortingUpload = uploadId;
    try {
      await s3AbortMultipartUpload(s3ConnectionId, key, uploadId);
      multipartUploads = multipartUploads.filter(u => u.upload_id !== uploadId);
    } catch (err: unknown) {
      uploadsError = String(err);
    } finally {
      abortingUpload = null;
    }
  }

  async function abortAllUploads() {
    if (!confirm(`Abort all ${multipartUploads.length} incomplete uploads?`)) return;
    abortingAll = true;
    uploadsError = '';
    try {
      for (const u of multipartUploads) {
        await s3AbortMultipartUpload(s3ConnectionId, u.key, u.upload_id);
      }
      multipartUploads = [];
    } catch (err: unknown) {
      uploadsError = String(err);
      multipartUploads = await s3ListMultipartUploads(s3ConnectionId);
    } finally {
      abortingAll = false;
    }
  }

  // ── Bucket-level: Lifecycle functions ────────────────────────────────────

  async function loadLifecycleRules() {
    if (lifecycleLoading) return;
    lifecycleExpanded = !lifecycleExpanded;
    if (!lifecycleExpanded || lifecycleRules.length > 0) return;
    lifecycleLoading = true;
    lifecycleMessage = '';
    try {
      lifecycleRules = await s3GetBucketLifecycle(s3ConnectionId);
      lifecycleOriginal = JSON.stringify(lifecycleRules);
    } catch (err: unknown) {
      lifecycleMessage = 'Error: ' + String(err);
    } finally {
      lifecycleLoading = false;
    }
  }

  function addLifecycleRule() {
    lifecycleRules = [...lifecycleRules, {
      id: `rule-${lifecycleRules.length + 1}`,
      prefix: '',
      enabled: true,
      transitions: [],
      expiration_days: null,
      noncurrent_transitions: [],
      noncurrent_expiration_days: null,
      abort_incomplete_days: null,
    }];
    editingRuleIndex = lifecycleRules.length - 1;
  }

  function removeLifecycleRule(index: number) {
    lifecycleRules = lifecycleRules.filter((_, i) => i !== index);
    if (editingRuleIndex === index) editingRuleIndex = null;
    else if (editingRuleIndex !== null && editingRuleIndex > index) editingRuleIndex--;
  }

  function addTransition(ruleIndex: number) {
    const rule = lifecycleRules[ruleIndex];
    rule.transitions = [...rule.transitions, { days: 30, storage_class: 'STANDARD_IA' }];
    lifecycleRules = [...lifecycleRules];
  }

  function removeTransition(ruleIndex: number, tIndex: number) {
    const rule = lifecycleRules[ruleIndex];
    rule.transitions = rule.transitions.filter((_, i) => i !== tIndex);
    lifecycleRules = [...lifecycleRules];
  }

  function addNoncurrentTransition(ruleIndex: number) {
    const rule = lifecycleRules[ruleIndex];
    rule.noncurrent_transitions = [...rule.noncurrent_transitions, { days: 30, storage_class: 'STANDARD_IA' }];
    lifecycleRules = [...lifecycleRules];
  }

  function removeNoncurrentTransition(ruleIndex: number, tIndex: number) {
    const rule = lifecycleRules[ruleIndex];
    rule.noncurrent_transitions = rule.noncurrent_transitions.filter((_, i) => i !== tIndex);
    lifecycleRules = [...lifecycleRules];
  }

  async function saveLifecycleRules() {
    savingLifecycle = true;
    lifecycleMessage = '';
    try {
      await s3PutBucketLifecycle(s3ConnectionId, lifecycleRules);
      lifecycleOriginal = JSON.stringify(lifecycleRules);
      lifecycleMessage = 'Lifecycle rules saved';
    } catch (err: unknown) {
      lifecycleMessage = 'Error: ' + String(err);
    } finally {
      savingLifecycle = false;
    }
  }

  function lifecycleSummary(rule: S3LifecycleRule): string {
    const parts: string[] = [];
    if (rule.transitions.length > 0) {
      parts.push(`${rule.transitions.length} transition(s)`);
    }
    if (rule.expiration_days !== null) {
      parts.push(`expire ${rule.expiration_days}d`);
    }
    if (rule.noncurrent_transitions.length > 0) {
      parts.push(`${rule.noncurrent_transitions.length} noncurrent transition(s)`);
    }
    if (rule.noncurrent_expiration_days !== null) {
      parts.push(`noncurrent expire ${rule.noncurrent_expiration_days}d`);
    }
    if (rule.abort_incomplete_days !== null) {
      parts.push(`abort incomplete ${rule.abort_incomplete_days}d`);
    }
    return parts.length > 0 ? parts.join(', ') : 'No actions configured';
  }

  // ── Bucket-level: CORS functions ────────────────────────────────────────

  async function loadCorsRules() {
    if (corsLoading) return;
    corsExpanded = !corsExpanded;
    if (!corsExpanded || corsRules.length > 0 || corsOriginal) return;
    corsLoading = true;
    corsMessage = '';
    try {
      corsRules = await s3GetBucketCors(s3ConnectionId);
      corsOriginal = JSON.stringify(corsRules);
    } catch (err: unknown) {
      corsMessage = 'Error: ' + String(err);
    } finally {
      corsLoading = false;
    }
  }

  function addCorsRule() {
    corsRules = [...corsRules, {
      allowed_origins: ['*'],
      allowed_methods: ['GET'],
      allowed_headers: ['*'],
      expose_headers: [],
      max_age_seconds: null,
    }];
  }

  function removeCorsRule(index: number) {
    corsRules = corsRules.filter((_, i) => i !== index);
  }

  function toggleCorsMethod(ruleIndex: number, method: string) {
    const rule = corsRules[ruleIndex];
    if (rule.allowed_methods.includes(method)) {
      rule.allowed_methods = rule.allowed_methods.filter(m => m !== method);
    } else {
      rule.allowed_methods = [...rule.allowed_methods, method];
    }
    corsRules = [...corsRules];
  }

  async function saveCorsRules() {
    savingCors = true;
    corsMessage = '';
    try {
      await s3PutBucketCors(s3ConnectionId, corsRules);
      corsOriginal = JSON.stringify(corsRules);
      corsMessage = 'CORS saved';
    } catch (err: unknown) {
      corsMessage = 'Error: ' + String(err);
    } finally {
      savingCors = false;
    }
  }

  // ── Bucket-level: Public Access Block functions ────────────────────────

  async function loadPublicAccessBlock() {
    publicAccessLoading = true;
    publicAccessMessage = '';
    try {
      publicAccessBlock = await s3GetPublicAccessBlock(s3ConnectionId);
      pabBlockPublicAcls = publicAccessBlock.block_public_acls;
      pabIgnorePublicAcls = publicAccessBlock.ignore_public_acls;
      pabBlockPublicPolicy = publicAccessBlock.block_public_policy;
      pabRestrictPublicBuckets = publicAccessBlock.restrict_public_buckets;
      pabOriginal = JSON.stringify({ a: pabBlockPublicAcls, b: pabIgnorePublicAcls, c: pabBlockPublicPolicy, d: pabRestrictPublicBuckets });
    } catch (err: unknown) {
      publicAccessMessage = 'Error: ' + String(err);
    } finally {
      publicAccessLoading = false;
    }
  }

  async function savePublicAccessBlock() {
    savingPublicAccess = true;
    publicAccessMessage = '';
    try {
      await s3PutPublicAccessBlock(s3ConnectionId, {
        block_public_acls: pabBlockPublicAcls,
        ignore_public_acls: pabIgnorePublicAcls,
        block_public_policy: pabBlockPublicPolicy,
        restrict_public_buckets: pabRestrictPublicBuckets,
      });
      pabOriginal = JSON.stringify({ a: pabBlockPublicAcls, b: pabIgnorePublicAcls, c: pabBlockPublicPolicy, d: pabRestrictPublicBuckets });
      publicAccessMessage = 'Public access block saved';
    } catch (err: unknown) {
      publicAccessMessage = 'Error: ' + String(err);
    } finally {
      savingPublicAccess = false;
    }
  }

  // ── Bucket-level: Bucket Policy functions ─────────────────────────────

  async function loadBucketPolicy() {
    if (policyLoading) return;
    policyExpanded = !policyExpanded;
    if (!policyExpanded || policyOriginal) return;
    policyLoading = true;
    policyMessage = '';
    try {
      const raw = await s3GetBucketPolicy(s3ConnectionId);
      if (raw) {
        try {
          policyText = JSON.stringify(JSON.parse(raw), null, 2);
        } catch {
          policyText = raw;
        }
      } else {
        policyText = '';
      }
      policyOriginal = policyText;
      policyJsonValid = true;
    } catch (err: unknown) {
      policyMessage = 'Error: ' + String(err);
    } finally {
      policyLoading = false;
    }
  }

  function handlePolicyInput(e: Event) {
    policyText = (e.target as HTMLTextAreaElement).value;
    if (policyText.trim() === '') {
      policyJsonValid = true;
    } else {
      try {
        JSON.parse(policyText);
        policyJsonValid = true;
      } catch {
        policyJsonValid = false;
      }
    }
  }

  async function saveBucketPolicy() {
    savingPolicy = true;
    policyMessage = '';
    try {
      await s3PutBucketPolicy(s3ConnectionId, policyText.trim());
      policyOriginal = policyText;
      policyMessage = policyText.trim() ? 'Policy saved' : 'Policy deleted';
    } catch (err: unknown) {
      policyMessage = 'Error: ' + String(err);
    } finally {
      savingPolicy = false;
    }
  }

  // ── Bucket-level: ACL functions ───────────────────────────────────────

  async function loadBucketAcl() {
    if (aclLoading) return;
    aclExpanded = !aclExpanded;
    if (!aclExpanded || bucketAcl) return;
    aclLoading = true;
    aclError = '';
    try {
      bucketAcl = await s3GetBucketAcl(s3ConnectionId);
    } catch (err: unknown) {
      aclError = String(err);
    } finally {
      aclLoading = false;
    }
  }

  function friendlyGrantee(grant: import('$lib/types').S3AclGrant): string {
    if (grant.grantee_uri) {
      switch (grant.grantee_uri) {
        case 'http://acs.amazonaws.com/groups/global/AllUsers':
          return 'Everyone (Public)';
        case 'http://acs.amazonaws.com/groups/global/AuthenticatedUsers':
          return 'Authenticated Users';
        case 'http://acs.amazonaws.com/groups/s3/LogDelivery':
          return 'Log Delivery';
        default:
          return grant.grantee_uri;
      }
    }
    if (grant.grantee_display_name) return grant.grantee_display_name;
    if (grant.grantee_email) return grant.grantee_email;
    if (grant.grantee_id) return grant.grantee_id.slice(0, 16) + '\u2026';
    return 'Unknown';
  }

  // ── Object-level: Metadata functions ────────────────────────────────────

  async function loadMetadata() {
    if (metadataLoading) return;
    metadataExpanded = !metadataExpanded;
    if (!metadataExpanded || objectMetadata) return;
    metadataLoading = true;
    metadataMessage = '';
    try {
      objectMetadata = await s3GetObjectMetadata(s3ConnectionId, path);
      metaContentType = objectMetadata.content_type ?? '';
      metaContentDisposition = objectMetadata.content_disposition ?? '';
      metaCacheControl = objectMetadata.cache_control ?? '';
      metaContentEncoding = objectMetadata.content_encoding ?? '';
      metaCustom = Object.entries(objectMetadata.custom).map(([key, value]) => ({ key, value }));
      metaOriginal = JSON.stringify({ ct: metaContentType, cd: metaContentDisposition, cc: metaCacheControl, ce: metaContentEncoding, custom: metaCustom });
    } catch (err: unknown) {
      metadataMessage = 'Error: ' + String(err);
    } finally {
      metadataLoading = false;
    }
  }

  function addCustomMeta() {
    metaCustom = [...metaCustom, { key: '', value: '' }];
  }

  function removeCustomMeta(index: number) {
    metaCustom = metaCustom.filter((_, i) => i !== index);
  }

  async function saveMetadata() {
    savingMetadata = true;
    metadataMessage = '';
    try {
      const customMap: Record<string, string> = {};
      for (const m of metaCustom) {
        if (m.key.trim()) customMap[m.key.trim()] = m.value;
      }
      await s3PutObjectMetadata(
        s3ConnectionId, path,
        metaContentType || null,
        metaContentDisposition || null,
        metaCacheControl || null,
        metaContentEncoding || null,
        customMap,
      );
      metaOriginal = JSON.stringify({ ct: metaContentType, cd: metaContentDisposition, cc: metaCacheControl, ce: metaContentEncoding, custom: metaCustom });
      metadataMessage = 'Metadata saved';
    } catch (err: unknown) {
      metadataMessage = 'Error: ' + String(err);
    } finally {
      savingMetadata = false;
    }
  }

  // ── Object-level: Tags functions ────────────────────────────────────────

  async function loadObjectTags() {
    if (objTagsLoading) return;
    objTagsExpanded = !objTagsExpanded;
    if (!objTagsExpanded || objectTags.length > 0) return;
    objTagsLoading = true;
    objTagsMessage = '';
    try {
      objectTags = await s3GetObjectTags(s3ConnectionId, path);
      objTagsOriginal = JSON.stringify(objectTags);
    } catch (err: unknown) {
      objTagsMessage = 'Error: ' + String(err);
    } finally {
      objTagsLoading = false;
    }
  }

  function addObjectTag() {
    if (objectTags.length >= 10) return;
    objectTags = [...objectTags, { key: '', value: '' }];
  }

  function removeObjectTag(index: number) {
    objectTags = objectTags.filter((_, i) => i !== index);
  }

  async function saveObjectTags() {
    savingObjTags = true;
    objTagsMessage = '';
    try {
      const filtered = objectTags.filter(t => t.key.trim());
      await s3PutObjectTags(s3ConnectionId, path, filtered);
      objectTags = filtered;
      objTagsOriginal = JSON.stringify(objectTags);
      objTagsMessage = 'Tags saved';
    } catch (err: unknown) {
      objTagsMessage = 'Error: ' + String(err);
    } finally {
      savingObjTags = false;
    }
  }

  let overlayEl = $state<HTMLDivElement | null>(null);

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape' || e.key === 'Enter') {
      e.preventDefault();
      e.stopPropagation();
      onClose();
    }
  }

  // Detect bucket root: s3://bucket-name/ with no further prefix
  function isBucketRoot(p: string): boolean {
    const match = p.match(/^s3:\/\/[^/]+\/$/);
    return !!match;
  }

  onMount(async () => {
    overlayEl?.focus();
    try {
      if (backend === 's3') {
        if (isBucketRoot(path)) {
          s3IsBucketRoot = true;
          s3IsPrefix = true;
          // Load bucket-level info
          bucketVersioningLoading = true;
          bucketEncryptionLoading = true;
          publicAccessLoading = true;
          Promise.all([
            s3GetBucketVersioning(s3ConnectionId).then(v => { bucketVersioning = v; }),
            s3GetBucketEncryption(s3ConnectionId).then(e => { bucketEncryption = e; }),
            loadPublicAccessBlock(),
          ]).catch(() => {}).finally(() => {
            bucketVersioningLoading = false;
            bucketEncryptionLoading = false;
          });
        } else if (path.endsWith('/')) {
          s3IsPrefix = true;
        } else {
          s3Props = await s3HeadObject(s3ConnectionId, path);
          selectedStorageClass = s3Props.storage_class ?? 'STANDARD';
        }
      } else {
        fileProps = await getFileProperties(path);
        editMode = fileProps.permissions & 0o777;
        if (fileProps.is_dir) {
          dirSizeLoading = true;
          getDirectorySize(fileProps.path)
            .then((size) => {
              dirSize = size;
            })
            .catch(() => {})
            .finally(() => {
              dirSizeLoading = false;
            });
        }
      }
    } catch (err: unknown) {
      error = String(err);
    } finally {
      loading = false;
    }
  });
</script>

<div
  class="dialog-overlay no-select"
  role="dialog"
  aria-modal="true"
  tabindex="-1"
  bind:this={overlayEl}
  onkeydown={handleKeydown}
>
  <div class="dialog-box">
    <div class="dialog-title">Properties</div>
    <div class="dialog-body">
      {#if loading}
        <div class="loading">Loading...</div>
      {:else if error}
        <div class="error">{error}</div>
      {:else if fileProps}
        <!-- Local file/directory properties -->
        <table class="props-table">
          <tbody>
            <tr><td class="prop-label">Name</td><td class="prop-value">{fileProps.name}</td></tr>
            <tr><td class="prop-label">Path</td><td class="prop-value path">{fileProps.path}</td></tr>
            <tr><td class="prop-label">Kind</td><td class="prop-value">{fileProps.kind}</td></tr>
            <tr>
              <td class="prop-label">Size</td>
              <td class="prop-value">
                {#if fileProps.is_dir}
                  {#if dirSizeLoading}
                    Calculating...
                  {:else if dirSize !== null}
                    {formatSize(dirSize)} ({dirSize.toLocaleString()} bytes)
                  {:else}
                    --
                  {/if}
                {:else}
                  {formatSize(fileProps.size)} ({fileProps.size.toLocaleString()} bytes)
                {/if}
              </td>
            </tr>
            <tr><td class="prop-label">Created</td><td class="prop-value">{formatDate(fileProps.created)}</td></tr>
            <tr><td class="prop-label">Modified</td><td class="prop-value">{formatDate(fileProps.modified)}</td></tr>
            <tr><td class="prop-label">Accessed</td><td class="prop-value">{formatDate(fileProps.accessed)}</td></tr>
            <tr><td class="prop-label">Owner</td><td class="prop-value">{fileProps.owner}</td></tr>
            <tr><td class="prop-label">Group</td><td class="prop-value">{fileProps.group}</td></tr>
            {#if fileProps.is_symlink && fileProps.symlink_target}
              <tr><td class="prop-label">Target</td><td class="prop-value path">{fileProps.symlink_target}</td></tr>
            {/if}
          </tbody>
        </table>

        <!-- Permissions editor -->
        <div class="section-title">Permissions</div>
        <div class="perms-section">
          <div class="octal-row">
            <span class="perm-display">{formatPermissions(editMode)}</span>
            <input
              class="octal-input"
              type="text"
              value={octalString()}
              maxlength="4"
              oninput={handleOctalInput}
            />
          </div>
          <div class="rwx-grid">
            {#each ['Owner', 'Group', 'Other'] as rowLabel}
              <div class="rwx-row">
                <span class="rwx-label">{rowLabel}</span>
                {#each permBits.filter((b) => b.row === rowLabel) as pb}
                  <label class="rwx-checkbox" class:checked={hasBit(pb.bit)}>
                    <input
                      type="checkbox"
                      checked={hasBit(pb.bit)}
                      onchange={() => toggleBit(pb.bit)}
                    />
                    {pb.label}
                  </label>
                {/each}
              </div>
            {/each}
          </div>
          {#if permsDirty}
            <button class="dialog-btn apply-btn" onclick={applyPermissions} disabled={applyingPerms}>
              {applyingPerms ? 'Applying...' : 'Apply'}
            </button>
          {/if}
        </div>
      {:else if s3Props}
        <!-- S3 object properties -->
        <table class="props-table">
          <tbody>
            <tr><td class="prop-label">Key</td><td class="prop-value path">{s3Props.key}</td></tr>
            <tr><td class="prop-label">Size</td><td class="prop-value">{formatSize(s3Props.size)} ({s3Props.size.toLocaleString()} bytes)</td></tr>
            <tr><td class="prop-label">Last Modified</td><td class="prop-value">{formatDate(s3Props.modified)}</td></tr>
            <tr><td class="prop-label">Content Type</td><td class="prop-value">{s3Props.content_type ?? '--'}</td></tr>
            <tr><td class="prop-label">ETag</td><td class="prop-value mono">{s3Props.etag ?? '--'}</td></tr>
            {#if s3Props.version_id}
              <tr><td class="prop-label">Version ID</td><td class="prop-value mono">{s3Props.version_id}</td></tr>
            {/if}
          </tbody>
        </table>

        <!-- Storage Class editor -->
        <div class="section-title">Storage Class</div>
        <div class="storage-class-section">
          <div class="sc-row">
            <select class="sc-select" bind:value={selectedStorageClass}>
              {#each storageClasses as sc}
                <option value={sc}>{sc}</option>
              {/each}
            </select>
            <button
              class="dialog-btn apply-btn"
              onclick={applyStorageClass}
              disabled={applyingClass || selectedStorageClass === s3Props.storage_class}
            >
              {applyingClass ? 'Applying...' : 'Apply'}
            </button>
          </div>
          {#if classMessage}
            <div class="sc-message" class:sc-error={classMessage.startsWith('Error')}>{classMessage}</div>
          {/if}
        </div>

        <!-- Glacier restore (only for glacier classes) -->
        {#if isGlacier}
          <div class="section-title">Glacier Restore</div>
          <div class="glacier-section">
            {#if s3Props.restore_status}
              <div class="restore-status">Restore status: {s3Props.restore_status}</div>
            {/if}
            <div class="glacier-row">
              <label class="glacier-label">
                Days:
                <input class="glacier-input" type="number" min="1" max="365" bind:value={restoreDays} />
              </label>
              <label class="glacier-label">
                Tier:
                <select class="glacier-select" bind:value={restoreTier}>
                  <option value="Standard">Standard</option>
                  <option value="Bulk">Bulk</option>
                  <option value="Expedited">Expedited</option>
                </select>
              </label>
              <button class="dialog-btn apply-btn" onclick={restoreFromGlacier} disabled={restoringGlacier}>
                {restoringGlacier ? 'Restoring...' : 'Restore'}
              </button>
            </div>
            {#if restoreMessage}
              <div class="sc-message" class:sc-error={restoreMessage.startsWith('Error')}>{restoreMessage}</div>
            {/if}
          </div>
        {/if}

        <!-- Versions section -->
        <button class="section-title versions-toggle" onclick={loadVersions}>
          Versions {versionsExpanded ? '\u25B4' : '\u25BE'}
        </button>
        {#if versionsExpanded}
          <div class="versions-section">
            {#if versionsLoading}
              <div class="loading">Loading versions...</div>
            {:else if versionsError}
              <div class="error">{versionsError}</div>
            {:else if versions.length === 0}
              <div class="versions-empty">No version history (versioning may not be enabled on this bucket)</div>
            {:else}
              <div class="versions-list">
                {#each versions as ver}
                  <div class="version-row" class:version-latest={ver.is_latest} class:version-delete-marker={ver.is_delete_marker}>
                    <div class="version-info">
                      <span class="version-id mono" title={ver.version_id}>{truncateVid(ver.version_id)}</span>
                      <span class="version-date">{formatDate(ver.modified)}</span>
                      {#if !ver.is_delete_marker}
                        <span class="version-size">{formatSize(ver.size)}</span>
                      {/if}
                      {#if ver.is_latest}
                        <span class="version-badge latest">Latest</span>
                      {/if}
                      {#if ver.is_delete_marker}
                        <span class="version-badge delete-marker">Delete Marker</span>
                      {/if}
                    </div>
                    <div class="version-actions">
                      {#if !ver.is_delete_marker}
                        <button
                          class="version-action-btn"
                          onclick={() => handleDownloadVersion(ver.version_id)}
                          disabled={versionActionLoading === ver.version_id}
                          title="Download this version"
                        >DL</button>
                        {#if !ver.is_latest}
                          <button
                            class="version-action-btn"
                            onclick={() => handleRestoreVersion(ver.version_id)}
                            disabled={versionActionLoading === ver.version_id}
                            title="Restore as current"
                          >Restore</button>
                        {/if}
                      {/if}
                      <button
                        class="version-action-btn danger"
                        onclick={() => handleDeleteVersion(ver.version_id)}
                        disabled={versionActionLoading === ver.version_id}
                        title="Delete this version"
                      >Del</button>
                    </div>
                  </div>
                {/each}
              </div>
            {/if}
          </div>
        {/if}

        <!-- Object Metadata section -->
        <button class="section-title versions-toggle" onclick={loadMetadata}>
          Metadata {metadataExpanded ? '\u25B4' : '\u25BE'}
        </button>
        {#if metadataExpanded}
          <div class="versions-section">
            {#if metadataLoading}
              <div class="loading">Loading metadata...</div>
            {:else if metadataMessage && !objectMetadata}
              <div class="error">{metadataMessage}</div>
            {:else}
              <div class="tag-editor">
                <label class="meta-field">
                  <span class="meta-label">Content-Type</span>
                  <input class="meta-input" type="text" bind:value={metaContentType} placeholder="application/octet-stream" />
                </label>
                <label class="meta-field">
                  <span class="meta-label">Content-Disposition</span>
                  <input class="meta-input" type="text" bind:value={metaContentDisposition} placeholder="inline" />
                </label>
                <label class="meta-field">
                  <span class="meta-label">Cache-Control</span>
                  <input class="meta-input" type="text" bind:value={metaCacheControl} placeholder="max-age=3600" />
                </label>
                <label class="meta-field">
                  <span class="meta-label">Content-Encoding</span>
                  <input class="meta-input" type="text" bind:value={metaContentEncoding} placeholder="gzip" />
                </label>
                <div class="tag-header">
                  <span class="meta-label">Custom Metadata</span>
                  <button class="version-action-btn" onclick={addCustomMeta}>+ Add</button>
                </div>
                {#each metaCustom as meta, i}
                  <div class="tag-row">
                    <input class="tag-input" type="text" bind:value={meta.key} placeholder="key" />
                    <input class="tag-input" type="text" bind:value={meta.value} placeholder="value" />
                    <button class="version-action-btn danger" onclick={() => removeCustomMeta(i)} title="Remove">&times;</button>
                  </div>
                {/each}
                <div class="tag-actions">
                  {#if metadataDirty}
                    <button class="dialog-btn apply-btn" onclick={saveMetadata} disabled={savingMetadata}>
                      {savingMetadata ? 'Saving...' : 'Save'}
                    </button>
                  {/if}
                  {#if metadataMessage}
                    <span class="sc-message" class:sc-error={metadataMessage.startsWith('Error')}>{metadataMessage}</span>
                  {/if}
                </div>
              </div>
            {/if}
          </div>
        {/if}

        <!-- Object Tags section -->
        <button class="section-title versions-toggle" onclick={loadObjectTags}>
          Tags {objTagsExpanded ? '\u25B4' : '\u25BE'}
        </button>
        {#if objTagsExpanded}
          <div class="versions-section">
            {#if objTagsLoading}
              <div class="loading">Loading tags...</div>
            {:else}
              <div class="tag-editor">
                {#each objectTags as tag, i}
                  <div class="tag-row">
                    <input class="tag-input" type="text" bind:value={tag.key} placeholder="key" />
                    <input class="tag-input" type="text" bind:value={tag.value} placeholder="value" />
                    <button class="version-action-btn danger" onclick={() => removeObjectTag(i)} title="Remove">&times;</button>
                  </div>
                {/each}
                {#if objectTags.length === 0}
                  <div class="versions-empty">No tags</div>
                {/if}
                <div class="tag-actions">
                  <button class="version-action-btn" onclick={addObjectTag} disabled={objectTags.length >= 10}>+ Add Tag</button>
                  {#if objTagsDirty}
                    <button class="dialog-btn apply-btn" onclick={saveObjectTags} disabled={savingObjTags}>
                      {savingObjTags ? 'Saving...' : 'Save'}
                    </button>
                  {/if}
                  {#if objTagsMessage}
                    <span class="sc-message" class:sc-error={objTagsMessage.startsWith('Error')}>{objTagsMessage}</span>
                  {/if}
                </div>
                {#if objectTags.length >= 10}
                  <div class="versions-empty">Maximum 10 tags reached</div>
                {/if}
              </div>
            {/if}
          </div>
        {/if}
      {:else if s3IsPrefix}
        <!-- S3 prefix (virtual directory) -->
        <table class="props-table">
          <tbody>
            <tr><td class="prop-label">{s3IsBucketRoot ? 'Bucket' : 'Prefix'}</td><td class="prop-value path">{path}</td></tr>
            <tr><td class="prop-label">Kind</td><td class="prop-value">{s3IsBucketRoot ? 'S3 Bucket' : 'S3 Prefix (virtual directory)'}</td></tr>
          </tbody>
        </table>

        {#if s3IsBucketRoot}
          <!-- Bucket Versioning -->
          <div class="section-title">Versioning</div>
          <div class="storage-class-section">
            {#if bucketVersioningLoading}
              <div class="loading">Loading...</div>
            {:else if bucketVersioning}
              <div class="sc-row">
                <span class="meta-label">Status: <strong>{bucketVersioning.status}</strong></span>
                <button
                  class="dialog-btn apply-btn"
                  onclick={toggleBucketVersioning}
                  disabled={applyingVersioning}
                >
                  {applyingVersioning ? 'Applying...' : bucketVersioning.status === 'Enabled' ? 'Suspend' : 'Enable'}
                </button>
              </div>
              {#if bucketVersioning.mfa_delete === 'Enabled'}
                <div class="restore-status">MFA Delete: Enabled</div>
              {/if}
              {#if versioningMessage}
                <div class="sc-message" class:sc-error={versioningMessage.startsWith('Error')}>{versioningMessage}</div>
              {/if}
            {/if}
          </div>

          <!-- Bucket Encryption -->
          <div class="section-title">Encryption</div>
          <div class="storage-class-section">
            {#if bucketEncryptionLoading}
              <div class="loading">Loading...</div>
            {:else if bucketEncryption && bucketEncryption.rules.length > 0}
              <table class="props-table">
                <tbody>
                  {#each bucketEncryption.rules as rule, i}
                    <tr><td class="prop-label">Algorithm</td><td class="prop-value">{rule.sse_algorithm}</td></tr>
                    {#if rule.kms_key_id}
                      <tr><td class="prop-label">KMS Key</td><td class="prop-value mono">{rule.kms_key_id}</td></tr>
                    {/if}
                    <tr><td class="prop-label">Bucket Key</td><td class="prop-value">{rule.bucket_key_enabled ? 'Enabled' : 'Disabled'}</td></tr>
                  {/each}
                </tbody>
              </table>
            {:else}
              <div class="versions-empty">No encryption configuration</div>
            {/if}
          </div>

          <!-- Public Access Block -->
          <div class="section-title">Public Access Block</div>
          <div class="storage-class-section">
            {#if publicAccessLoading}
              <div class="loading">Loading...</div>
            {:else if publicAccessMessage && !publicAccessBlock}
              <div class="error">{publicAccessMessage}</div>
            {:else if publicAccessBlock}
              <div class="pab-section">
                <label class="pab-checkbox">
                  <input type="checkbox" bind:checked={pabBlockPublicAcls} />
                  <span>Block public ACLs</span>
                </label>
                <label class="pab-checkbox">
                  <input type="checkbox" bind:checked={pabIgnorePublicAcls} />
                  <span>Ignore public ACLs</span>
                </label>
                <label class="pab-checkbox">
                  <input type="checkbox" bind:checked={pabBlockPublicPolicy} />
                  <span>Block public policy</span>
                </label>
                <label class="pab-checkbox">
                  <input type="checkbox" bind:checked={pabRestrictPublicBuckets} />
                  <span>Restrict public buckets</span>
                </label>
                <div class="tag-actions">
                  {#if publicAccessDirty}
                    <button class="dialog-btn apply-btn" onclick={savePublicAccessBlock} disabled={savingPublicAccess}>
                      {savingPublicAccess ? 'Saving...' : 'Save'}
                    </button>
                  {/if}
                  {#if publicAccessMessage}
                    <span class="sc-message" class:sc-error={publicAccessMessage.startsWith('Error')}>{publicAccessMessage}</span>
                  {/if}
                </div>
              </div>
            {/if}
          </div>

          <!-- Bucket Tags -->
          <button class="section-title versions-toggle" onclick={loadBucketTags}>
            Bucket Tags {bucketTagsExpanded ? '\u25B4' : '\u25BE'}
          </button>
          {#if bucketTagsExpanded}
            <div class="versions-section">
              {#if bucketTagsLoading}
                <div class="loading">Loading tags...</div>
              {:else}
                <div class="tag-editor">
                  {#each bucketTags as tag, i}
                    <div class="tag-row">
                      <input class="tag-input" type="text" bind:value={tag.key} placeholder="key" />
                      <input class="tag-input" type="text" bind:value={tag.value} placeholder="value" />
                      <button class="version-action-btn danger" onclick={() => removeBucketTag(i)} title="Remove">&times;</button>
                    </div>
                  {/each}
                  {#if bucketTags.length === 0}
                    <div class="versions-empty">No tags</div>
                  {/if}
                  <div class="tag-actions">
                    <button class="version-action-btn" onclick={addBucketTag} disabled={bucketTags.length >= 50}>+ Add Tag</button>
                    {#if bucketTagsDirty}
                      <button class="dialog-btn apply-btn" onclick={saveBucketTags} disabled={savingBucketTags}>
                        {savingBucketTags ? 'Saving...' : 'Save'}
                      </button>
                    {/if}
                    {#if bucketTagsMessage}
                      <span class="sc-message" class:sc-error={bucketTagsMessage.startsWith('Error')}>{bucketTagsMessage}</span>
                    {/if}
                  </div>
                </div>
              {/if}
            </div>
          {/if}

          <!-- Incomplete Uploads -->
          <button class="section-title versions-toggle" onclick={loadMultipartUploads}>
            Incomplete Uploads {uploadsExpanded ? '\u25B4' : '\u25BE'}
          </button>
          {#if uploadsExpanded}
            <div class="versions-section">
              {#if uploadsLoading}
                <div class="loading">Loading uploads...</div>
              {:else if uploadsError}
                <div class="error">{uploadsError}</div>
              {:else if multipartUploads.length === 0}
                <div class="versions-empty">No incomplete multipart uploads</div>
              {:else}
                <div class="versions-list">
                  <div class="tag-actions" style="margin-bottom: 4px;">
                    <button class="version-action-btn danger" onclick={abortAllUploads} disabled={abortingAll}>
                      {abortingAll ? 'Aborting...' : 'Abort All'}
                    </button>
                  </div>
                  {#each multipartUploads as upload}
                    <div class="version-row">
                      <div class="version-info">
                        <span class="version-id mono" title={upload.key}>{upload.key.length > 40 ? upload.key.slice(0, 40) + '\u2026' : upload.key}</span>
                        <span class="version-date">{formatDate(upload.initiated)}</span>
                      </div>
                      <div class="version-actions">
                        <button
                          class="version-action-btn danger"
                          onclick={() => abortUpload(upload.key, upload.upload_id)}
                          disabled={abortingUpload === upload.upload_id}
                          title="Abort this upload"
                        >Abort</button>
                      </div>
                    </div>
                  {/each}
                </div>
              {/if}
            </div>
          {/if}

          <!-- Lifecycle Rules -->
          <button class="section-title versions-toggle" onclick={loadLifecycleRules}>
            Lifecycle Rules {lifecycleExpanded ? '\u25B4' : '\u25BE'}
          </button>
          {#if lifecycleExpanded}
            <div class="versions-section">
              {#if lifecycleLoading}
                <div class="loading">Loading lifecycle rules...</div>
              {:else}
                <div class="tag-editor">
                  {#each lifecycleRules as rule, i}
                    <div class="lifecycle-rule" class:lifecycle-disabled={!rule.enabled}>
                      <div class="lifecycle-rule-header">
                        <label class="lifecycle-enabled-label">
                          <input type="checkbox" bind:checked={rule.enabled} onchange={() => { lifecycleRules = [...lifecycleRules]; }} />
                        </label>
                        <span class="lifecycle-rule-id" title={rule.id}>{rule.id || '(no id)'}</span>
                        {#if editingRuleIndex !== i}
                          <span class="lifecycle-summary">{lifecycleSummary(rule)}</span>
                        {/if}
                        <div class="lifecycle-rule-actions">
                          <button class="version-action-btn" onclick={() => { editingRuleIndex = editingRuleIndex === i ? null : i; }}>
                            {editingRuleIndex === i ? 'Collapse' : 'Edit'}
                          </button>
                          <button class="version-action-btn danger" onclick={() => removeLifecycleRule(i)} title="Delete rule">&times;</button>
                        </div>
                      </div>

                      {#if editingRuleIndex === i}
                        <div class="lifecycle-rule-body">
                          <label class="meta-field">
                            <span class="meta-label">Rule ID</span>
                            <input class="meta-input" type="text" bind:value={rule.id} onchange={() => { lifecycleRules = [...lifecycleRules]; }} />
                          </label>
                          <label class="meta-field">
                            <span class="meta-label">Prefix Filter</span>
                            <input class="meta-input" type="text" bind:value={rule.prefix} placeholder="(all objects)" onchange={() => { lifecycleRules = [...lifecycleRules]; }} />
                          </label>

                          <!-- Current Version Transitions -->
                          <div class="tag-header">
                            <span class="meta-label">Transitions</span>
                            <button class="version-action-btn" onclick={() => addTransition(i)}>+ Add</button>
                          </div>
                          {#each rule.transitions as t, ti}
                            <div class="tag-row">
                              <label class="lifecycle-days-label">
                                Days:
                                <input class="lifecycle-days-input" type="number" min="0" bind:value={t.days} onchange={() => { lifecycleRules = [...lifecycleRules]; }} />
                              </label>
                              <select class="sc-select" bind:value={t.storage_class} onchange={() => { lifecycleRules = [...lifecycleRules]; }}>
                                {#each lifecycleStorageClasses as sc}
                                  <option value={sc}>{sc}</option>
                                {/each}
                              </select>
                              <button class="version-action-btn danger" onclick={() => removeTransition(i, ti)}>&times;</button>
                            </div>
                          {/each}

                          <!-- Expiration -->
                          <div class="tag-header">
                            <span class="meta-label">Expiration (days)</span>
                          </div>
                          <div class="tag-row">
                            <input
                              class="lifecycle-days-input"
                              type="number"
                              min="0"
                              value={rule.expiration_days ?? ''}
                              oninput={(e) => {
                                const v = (e.target as HTMLInputElement).value;
                                rule.expiration_days = v ? parseInt(v, 10) : null;
                                lifecycleRules = [...lifecycleRules];
                              }}
                              placeholder="none"
                            />
                          </div>

                          <!-- Noncurrent Version Transitions -->
                          <div class="tag-header">
                            <span class="meta-label">Noncurrent Version Transitions</span>
                            <button class="version-action-btn" onclick={() => addNoncurrentTransition(i)}>+ Add</button>
                          </div>
                          {#each rule.noncurrent_transitions as t, ti}
                            <div class="tag-row">
                              <label class="lifecycle-days-label">
                                Days:
                                <input class="lifecycle-days-input" type="number" min="0" bind:value={t.days} onchange={() => { lifecycleRules = [...lifecycleRules]; }} />
                              </label>
                              <select class="sc-select" bind:value={t.storage_class} onchange={() => { lifecycleRules = [...lifecycleRules]; }}>
                                {#each lifecycleStorageClasses as sc}
                                  <option value={sc}>{sc}</option>
                                {/each}
                              </select>
                              <button class="version-action-btn danger" onclick={() => removeNoncurrentTransition(i, ti)}>&times;</button>
                            </div>
                          {/each}

                          <!-- Noncurrent Version Expiration -->
                          <div class="tag-header">
                            <span class="meta-label">Noncurrent Version Expiration (days)</span>
                          </div>
                          <div class="tag-row">
                            <input
                              class="lifecycle-days-input"
                              type="number"
                              min="0"
                              value={rule.noncurrent_expiration_days ?? ''}
                              oninput={(e) => {
                                const v = (e.target as HTMLInputElement).value;
                                rule.noncurrent_expiration_days = v ? parseInt(v, 10) : null;
                                lifecycleRules = [...lifecycleRules];
                              }}
                              placeholder="none"
                            />
                          </div>

                          <!-- Abort Incomplete Multipart Upload -->
                          <div class="tag-header">
                            <span class="meta-label">Abort Incomplete Multipart (days)</span>
                          </div>
                          <div class="tag-row">
                            <input
                              class="lifecycle-days-input"
                              type="number"
                              min="0"
                              value={rule.abort_incomplete_days ?? ''}
                              oninput={(e) => {
                                const v = (e.target as HTMLInputElement).value;
                                rule.abort_incomplete_days = v ? parseInt(v, 10) : null;
                                lifecycleRules = [...lifecycleRules];
                              }}
                              placeholder="none"
                            />
                          </div>
                        </div>
                      {/if}
                    </div>
                  {/each}
                  {#if lifecycleRules.length === 0}
                    <div class="versions-empty">No lifecycle rules</div>
                  {/if}
                  <div class="tag-actions">
                    <button class="version-action-btn" onclick={addLifecycleRule}>+ Add Rule</button>
                    {#if lifecycleDirty}
                      <button class="dialog-btn apply-btn" onclick={saveLifecycleRules} disabled={savingLifecycle}>
                        {savingLifecycle ? 'Saving...' : 'Save'}
                      </button>
                    {/if}
                    {#if lifecycleMessage}
                      <span class="sc-message" class:sc-error={lifecycleMessage.startsWith('Error')}>{lifecycleMessage}</span>
                    {/if}
                  </div>
                </div>
              {/if}
            </div>
          {/if}

          <!-- CORS Configuration -->
          <button class="section-title versions-toggle" onclick={loadCorsRules}>
            CORS Configuration {corsExpanded ? '\u25B4' : '\u25BE'}
          </button>
          {#if corsExpanded}
            <div class="versions-section">
              {#if corsLoading}
                <div class="loading">Loading CORS rules...</div>
              {:else}
                <div class="tag-editor">
                  {#each corsRules as rule, i}
                    <div class="lifecycle-rule">
                      <div class="lifecycle-rule-header">
                        <span class="lifecycle-rule-id">Rule {i + 1}</span>
                        <div class="lifecycle-rule-actions">
                          <button class="version-action-btn danger" onclick={() => removeCorsRule(i)} title="Remove rule">&times;</button>
                        </div>
                      </div>
                      <div class="lifecycle-rule-body">
                        <label class="meta-field">
                          <span class="meta-label">Allowed Origins</span>
                          <textarea
                            class="cors-textarea"
                            value={rule.allowed_origins.join('\n')}
                            oninput={(e) => {
                              rule.allowed_origins = (e.target as HTMLTextAreaElement).value.split('\n').filter(s => s.trim());
                              corsRules = [...corsRules];
                            }}
                            placeholder="* or https://example.com (one per line)"
                            rows="2"
                          ></textarea>
                        </label>
                        <div class="tag-header">
                          <span class="meta-label">Allowed Methods</span>
                        </div>
                        <div class="cors-methods">
                          {#each corsMethods as method}
                            <label class="rwx-checkbox" class:checked={rule.allowed_methods.includes(method)}>
                              <input type="checkbox" checked={rule.allowed_methods.includes(method)} onchange={() => toggleCorsMethod(i, method)} />
                              {method}
                            </label>
                          {/each}
                        </div>
                        <label class="meta-field">
                          <span class="meta-label">Allowed Headers</span>
                          <input
                            class="meta-input"
                            type="text"
                            value={rule.allowed_headers.join(', ')}
                            oninput={(e) => {
                              rule.allowed_headers = (e.target as HTMLInputElement).value.split(',').map(s => s.trim()).filter(Boolean);
                              corsRules = [...corsRules];
                            }}
                            placeholder="* or comma-separated headers"
                          />
                        </label>
                        <label class="meta-field">
                          <span class="meta-label">Expose Headers</span>
                          <input
                            class="meta-input"
                            type="text"
                            value={rule.expose_headers.join(', ')}
                            oninput={(e) => {
                              rule.expose_headers = (e.target as HTMLInputElement).value.split(',').map(s => s.trim()).filter(Boolean);
                              corsRules = [...corsRules];
                            }}
                            placeholder="comma-separated headers"
                          />
                        </label>
                        <label class="meta-field">
                          <span class="meta-label">Max Age (sec)</span>
                          <input
                            class="lifecycle-days-input"
                            type="number"
                            min="0"
                            value={rule.max_age_seconds ?? ''}
                            oninput={(e) => {
                              const v = (e.target as HTMLInputElement).value;
                              rule.max_age_seconds = v ? parseInt(v, 10) : null;
                              corsRules = [...corsRules];
                            }}
                            placeholder="none"
                          />
                        </label>
                      </div>
                    </div>
                  {/each}
                  {#if corsRules.length === 0}
                    <div class="versions-empty">No CORS rules</div>
                  {/if}
                  <div class="tag-actions">
                    <button class="version-action-btn" onclick={addCorsRule}>+ Add Rule</button>
                    {#if corsDirty}
                      <button class="dialog-btn apply-btn" onclick={saveCorsRules} disabled={savingCors}>
                        {savingCors ? 'Saving...' : 'Save'}
                      </button>
                    {/if}
                    {#if corsMessage}
                      <span class="sc-message" class:sc-error={corsMessage.startsWith('Error')}>{corsMessage}</span>
                    {/if}
                  </div>
                </div>
              {/if}
            </div>
          {/if}

          <!-- Bucket Policy -->
          <button class="section-title versions-toggle" onclick={loadBucketPolicy}>
            Bucket Policy {policyExpanded ? '\u25B4' : '\u25BE'}
          </button>
          {#if policyExpanded}
            <div class="versions-section">
              {#if policyLoading}
                <div class="loading">Loading policy...</div>
              {:else}
                <div class="tag-editor">
                  <textarea
                    class="policy-editor"
                    value={policyText}
                    oninput={handlePolicyInput}
                    placeholder={'{"Version":"2012-10-17","Statement":[...]}'}
                    rows="12"
                  ></textarea>
                  {#if !policyJsonValid}
                    <div class="sc-message sc-error">Invalid JSON</div>
                  {/if}
                  <div class="tag-actions">
                    {#if policyDirty}
                      <button
                        class="dialog-btn apply-btn"
                        onclick={saveBucketPolicy}
                        disabled={savingPolicy || !policyJsonValid}
                      >
                        {savingPolicy ? 'Saving...' : policyText.trim() ? 'Save Policy' : 'Delete Policy'}
                      </button>
                    {/if}
                    {#if policyMessage}
                      <span class="sc-message" class:sc-error={policyMessage.startsWith('Error')}>{policyMessage}</span>
                    {/if}
                  </div>
                </div>
              {/if}
            </div>
          {/if}

          <!-- ACL (Read-Only) -->
          <button class="section-title versions-toggle" onclick={loadBucketAcl}>
            ACL {aclExpanded ? '\u25B4' : '\u25BE'}
          </button>
          {#if aclExpanded}
            <div class="versions-section">
              {#if aclLoading}
                <div class="loading">Loading ACL...</div>
              {:else if aclError}
                <div class="error">{aclError}</div>
              {:else if bucketAcl}
                <div class="tag-editor">
                  <div class="acl-owner">
                    <span class="meta-label">Owner:</span>
                    <span class="acl-owner-name">{bucketAcl.owner_display_name ?? bucketAcl.owner_id}</span>
                  </div>
                  {#if bucketAcl.grants.length === 0}
                    <div class="versions-empty">No grants</div>
                  {:else}
                    <div class="versions-list">
                      {#each bucketAcl.grants as grant}
                        <div class="version-row">
                          <div class="version-info">
                            <span class="acl-grantee">{friendlyGrantee(grant)}</span>
                          </div>
                          <span class="version-badge latest">{grant.permission}</span>
                        </div>
                      {/each}
                    </div>
                  {/if}
                  <div class="acl-note">ACL is read-only</div>
                </div>
              {/if}
            </div>
          {/if}
        {/if}
      {/if}

      <div class="dialog-buttons">
        <button class="dialog-btn primary" onclick={onClose}>Close</button>
      </div>
    </div>
  </div>
</div>

<style>
  .dialog-overlay {
    position: fixed;
    top: 0;
    left: 0;
    right: 0;
    bottom: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    background: rgba(0, 0, 0, 0.5);
    backdrop-filter: blur(8px);
    -webkit-backdrop-filter: blur(8px);
    z-index: 100;
  }

  .dialog-box {
    background: var(--dialog-bg);
    border: 1px solid var(--dialog-border);
    border-radius: var(--radius-lg);
    min-width: 50ch;
    max-width: 70ch;
    max-height: 85vh;
    box-shadow: var(--shadow-dialog);
    overflow: hidden;
    display: flex;
    flex-direction: column;
  }

  .dialog-title {
    background: transparent;
    color: var(--dialog-title-text);
    text-align: center;
    padding: 12px 16px;
    font-weight: 600;
    font-size: 14px;
    border-bottom: 1px solid var(--dialog-border);
    flex-shrink: 0;
  }

  .dialog-body {
    padding: 20px 24px;
    display: flex;
    flex-direction: column;
    gap: 12px;
    overflow-y: auto;
  }

  .loading, .error {
    text-align: center;
    padding: 20px;
    font-size: 13px;
    color: var(--text-secondary);
  }

  .error {
    color: var(--text-error, #ff6b6b);
  }

  .props-table {
    width: 100%;
    border-collapse: collapse;
  }

  .props-table td {
    padding: 4px 0;
    font-size: 13px;
    vertical-align: top;
  }

  .prop-label {
    color: var(--text-secondary);
    width: 110px;
    white-space: nowrap;
    padding-right: 12px;
  }

  .prop-value {
    color: var(--text-primary);
    word-break: break-all;
  }

  .prop-value.path {
    font-size: 12px;
    opacity: 0.85;
  }

  .prop-value.mono {
    font-family: var(--font-mono, monospace);
    font-size: 12px;
  }

  .section-title {
    font-size: 11px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.5px;
    color: var(--text-secondary);
    opacity: 0.7;
    padding-top: 4px;
  }

  .perms-section {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .octal-row {
    display: flex;
    align-items: center;
    gap: 12px;
  }

  .perm-display {
    font-family: var(--font-mono, monospace);
    font-size: 14px;
    color: var(--text-primary);
    letter-spacing: 1px;
  }

  .octal-input {
    width: 60px;
    padding: 4px 8px;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
    background: var(--bg-surface);
    color: var(--text-primary);
    font-family: var(--font-mono, monospace);
    font-size: 13px;
    text-align: center;
  }

  .octal-input:focus {
    outline: none;
    border-color: var(--border-active);
  }

  .rwx-grid {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .rwx-row {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .rwx-label {
    font-size: 12px;
    color: var(--text-secondary);
    width: 50px;
  }

  .rwx-checkbox {
    display: flex;
    align-items: center;
    gap: 3px;
    padding: 3px 8px;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
    font-size: 12px;
    font-family: var(--font-mono, monospace);
    color: var(--text-primary);
    cursor: pointer;
    transition: background var(--transition-fast), border-color var(--transition-fast);
  }

  .rwx-checkbox:hover {
    background: var(--bg-hover);
  }

  .rwx-checkbox.checked {
    border-color: var(--border-active);
    background: rgba(110, 168, 254, 0.1);
    color: var(--text-accent);
  }

  .rwx-checkbox input[type='checkbox'] {
    display: none;
  }

  .apply-btn {
    align-self: flex-start;
    padding: 6px 18px;
    background: rgba(110, 168, 254, 0.2);
    border: 1px solid var(--border-active);
    border-radius: var(--radius-sm);
    color: var(--text-accent);
    cursor: pointer;
    font-size: 12px;
    font-family: inherit;
    transition: background var(--transition-fast);
  }

  .apply-btn:hover {
    background: rgba(110, 168, 254, 0.3);
  }

  .apply-btn:disabled {
    opacity: 0.5;
    cursor: default;
  }

  .dialog-buttons {
    display: flex;
    justify-content: center;
    margin-top: 8px;
    flex-shrink: 0;
  }

  .dialog-btn {
    padding: 8px 24px;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
    background: var(--bg-surface);
    color: var(--text-primary);
    cursor: pointer;
    font-size: 13px;
    font-family: inherit;
    transition: background var(--transition-fast), border-color var(--transition-fast);
  }

  .dialog-btn:hover {
    background: var(--bg-hover);
    border-color: var(--text-accent);
  }

  .dialog-btn.primary {
    background: rgba(110, 168, 254, 0.2);
    border-color: var(--border-active);
    color: var(--text-accent);
  }

  .dialog-btn.primary:hover {
    background: rgba(110, 168, 254, 0.3);
  }

  /* Storage class section */
  .storage-class-section {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .sc-row {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .sc-select {
    flex: 1;
    padding: 5px 8px;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
    background: var(--bg-surface);
    color: var(--text-primary);
    font-size: 12px;
    font-family: inherit;
  }

  .sc-select:focus {
    outline: none;
    border-color: var(--border-active);
  }

  .sc-message {
    font-size: 12px;
    color: var(--success-color, #4ec990);
  }

  .sc-message.sc-error {
    color: var(--text-error, #ff6b6b);
  }

  /* Glacier section */
  .glacier-section {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .glacier-row {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
  }

  .glacier-label {
    font-size: 12px;
    color: var(--text-secondary);
    display: flex;
    align-items: center;
    gap: 4px;
  }

  .glacier-input {
    width: 60px;
    padding: 4px 6px;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
    background: var(--bg-surface);
    color: var(--text-primary);
    font-size: 12px;
    font-family: inherit;
    text-align: center;
  }

  .glacier-input:focus {
    outline: none;
    border-color: var(--border-active);
  }

  .glacier-select {
    padding: 4px 6px;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
    background: var(--bg-surface);
    color: var(--text-primary);
    font-size: 12px;
    font-family: inherit;
  }

  .restore-status {
    font-size: 12px;
    color: var(--text-secondary);
    font-style: italic;
  }

  /* Versions section */
  .versions-toggle {
    cursor: pointer;
    background: none;
    border: none;
    text-align: left;
    padding: 4px 0;
    width: 100%;
  }

  .versions-toggle:hover {
    opacity: 1;
  }

  .versions-section {
    max-height: 200px;
    overflow-y: auto;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
    padding: 4px;
  }

  .versions-empty {
    font-size: 12px;
    color: var(--text-secondary);
    text-align: center;
    padding: 8px;
  }

  .versions-list {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .version-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 4px 6px;
    border-radius: var(--radius-sm);
    font-size: 11px;
    transition: background var(--transition-fast);
  }

  .version-row:hover {
    background: var(--bg-hover);
  }

  .version-row.version-latest {
    background: rgba(110, 168, 254, 0.05);
  }

  .version-row.version-delete-marker {
    opacity: 0.6;
  }

  .version-info {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
    flex: 1;
  }

  .version-id {
    font-size: 10px;
    opacity: 0.7;
  }

  .version-date {
    color: var(--text-secondary);
    white-space: nowrap;
  }

  .version-size {
    color: var(--text-secondary);
    white-space: nowrap;
  }

  .version-badge {
    font-size: 9px;
    font-weight: 600;
    text-transform: uppercase;
    padding: 1px 4px;
    border-radius: 2px;
    white-space: nowrap;
  }

  .version-badge.latest {
    background: rgba(110, 168, 254, 0.2);
    color: var(--text-accent);
  }

  .version-badge.delete-marker {
    background: rgba(255, 107, 107, 0.2);
    color: var(--text-error, #ff6b6b);
  }

  .version-actions {
    display: flex;
    gap: 4px;
    flex-shrink: 0;
  }

  .version-action-btn {
    padding: 2px 6px;
    font-size: 10px;
    font-family: inherit;
    border: 1px solid var(--border-subtle);
    border-radius: 2px;
    background: var(--bg-surface);
    color: var(--text-primary);
    cursor: pointer;
    transition: background var(--transition-fast);
  }

  .version-action-btn:hover {
    background: var(--bg-hover);
    border-color: var(--border-active);
  }

  .version-action-btn.danger:hover {
    border-color: var(--text-error, #ff6b6b);
    color: var(--text-error, #ff6b6b);
  }

  .version-action-btn:disabled {
    opacity: 0.4;
    cursor: default;
  }

  /* Tag editor */
  .tag-editor {
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding: 4px 0;
  }

  .tag-row {
    display: flex;
    align-items: center;
    gap: 4px;
  }

  .tag-input {
    flex: 1;
    padding: 3px 6px;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
    background: var(--bg-surface);
    color: var(--text-primary);
    font-size: 11px;
    font-family: inherit;
  }

  .tag-input:focus {
    outline: none;
    border-color: var(--border-active);
  }

  .tag-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 4px 0 2px;
  }

  .tag-actions {
    display: flex;
    align-items: center;
    gap: 8px;
    padding-top: 4px;
  }

  /* Metadata fields */
  .meta-field {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .meta-label {
    font-size: 11px;
    color: var(--text-secondary);
    white-space: nowrap;
    min-width: 110px;
  }

  .meta-input {
    flex: 1;
    padding: 3px 6px;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
    background: var(--bg-surface);
    color: var(--text-primary);
    font-size: 11px;
    font-family: inherit;
  }

  .meta-input:focus {
    outline: none;
    border-color: var(--border-active);
  }

  /* Lifecycle Rules */

  .lifecycle-rule {
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
    padding: 6px;
    margin-bottom: 6px;
  }

  .lifecycle-rule.lifecycle-disabled {
    opacity: 0.6;
  }

  .lifecycle-rule-header {
    display: flex;
    align-items: center;
    gap: 6px;
    min-height: 24px;
  }

  .lifecycle-enabled-label {
    flex-shrink: 0;
  }

  .lifecycle-rule-id {
    font-size: 12px;
    font-weight: 600;
    color: var(--text-primary);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    max-width: 120px;
  }

  .lifecycle-summary {
    font-size: 11px;
    color: var(--text-secondary);
    flex: 1;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .lifecycle-rule-actions {
    display: flex;
    gap: 4px;
    flex-shrink: 0;
    margin-left: auto;
  }

  .lifecycle-rule-body {
    margin-top: 6px;
    padding-top: 6px;
    border-top: 1px solid var(--border-subtle);
  }

  .lifecycle-days-label {
    display: flex;
    align-items: center;
    gap: 4px;
    font-size: 11px;
    color: var(--text-secondary);
  }

  .lifecycle-days-input {
    width: 60px;
    padding: 2px 4px;
    font-size: 11px;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
    background: var(--bg-surface);
    color: var(--text-primary);
  }

  /* Public Access Block */
  .pab-section {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .pab-checkbox {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 12px;
    color: var(--text-primary);
    cursor: pointer;
  }

  /* CORS */
  .cors-textarea {
    width: 100%;
    padding: 4px 6px;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
    background: var(--bg-surface);
    color: var(--text-primary);
    font-size: 11px;
    font-family: inherit;
    resize: vertical;
  }

  .cors-textarea:focus {
    outline: none;
    border-color: var(--border-active);
  }

  .cors-methods {
    display: flex;
    gap: 4px;
    flex-wrap: wrap;
    padding: 2px 0;
  }

  /* Policy Editor */
  .policy-editor {
    width: 100%;
    padding: 6px 8px;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
    background: var(--bg-surface);
    color: var(--text-primary);
    font-size: 11px;
    font-family: var(--font-mono, monospace);
    resize: vertical;
    line-height: 1.4;
  }

  .policy-editor:focus {
    outline: none;
    border-color: var(--border-active);
  }

  /* ACL */
  .acl-owner {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 4px 0;
  }

  .acl-owner-name {
    font-size: 12px;
    color: var(--text-primary);
  }

  .acl-grantee {
    font-size: 11px;
    color: var(--text-primary);
  }

  .acl-note {
    font-size: 10px;
    color: var(--text-secondary);
    font-style: italic;
    text-align: center;
    padding: 4px 0;
  }
</style>
