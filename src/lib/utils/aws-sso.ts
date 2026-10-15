/** These tools must not change manual/custom S3 connection behavior. */
export function showAwsProfileTools(provider: string | undefined, endpoint: string | undefined, anonymous = false, oidc = false): boolean {
  return (provider ?? 'aws') === 'aws' && !endpoint?.trim() && !anonymous && !oidc;
}
export function allowSsoLogin(accessKey: string | undefined, secretKey: string | undefined): boolean {
  return !accessKey?.trim() && !secretKey?.trim();
}
export function isSsoSessionError(message: string): boolean {
  return /\[AWS_SSO_(SESSION_EXPIRED|SESSION_UNAVAILABLE|CONFIGURATION)\]/.test(message);
}
