import { CheckCircle2, Monitor, RefreshCw, Wifi } from 'lucide-react';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from '@/components/ui/card';
import { Skeleton } from '@/components/ui/skeleton';

function ProfileField({ label, children, className = '' }) {
  return (
    <div className={`profile-field ${className}`.trim()}>
      <dt>{label}</dt>
      <dd>{children}</dd>
    </div>
  );
}

function ProfileSkeleton() {
  return (
    <div className="profile-skeleton" aria-label="Loading computer profile">
      {Array.from({ length: 5 }, (_, index) => <Skeleton key={index} className="profile-skeleton-line" />)}
    </div>
  );
}

export function WorkerComputer({
  profile,
  profileLoaded,
  profileLoading,
  profileError,
  registration,
  refreshLoading,
  setupActive,
  onRefresh,
}) {
  return (
    <section className="computer-view" aria-label="Computer and connection details">
      <div className="computer-grid">
        <Card>
          <CardHeader className="computer-card-header">
            <div className="computer-card-heading">
              <span className="resource-icon" aria-hidden="true"><Monitor /></span>
              <div>
                <CardTitle className="text-base">This computer</CardTitle>
                <CardDescription>Detected local capacity</CardDescription>
              </div>
            </div>
            <Button type="button" variant="outline" size="sm" onClick={onRefresh} disabled={refreshLoading || setupActive}>
              <RefreshCw className={refreshLoading || setupActive ? 'animate-spin motion-reduce:animate-none' : ''} aria-hidden="true" />
              {refreshLoading || setupActive ? 'Checking' : 'Check again'}
            </Button>
          </CardHeader>
          <CardContent>
            {profileLoading && !profileLoaded ? <ProfileSkeleton /> : null}
            {profileError ? (
              <div className="computer-message is-error" role="alert">
                <strong>We could not prepare this computer.</strong>
                <span>Make sure the Worker app is open, then try again.</span>
                {!profileLoaded ? (
                  <Button type="button" variant="outline" onClick={onRefresh} disabled={refreshLoading || setupActive}>
                    {refreshLoading || setupActive ? 'Trying again' : 'Try again'}
                  </Button>
                ) : null}
              </div>
            ) : null}
            {profileLoaded ? (
              <dl className="profile-details">
                <ProfileField label="Computer ID" className="profile-field-wide">{profile.worker_id || '(not connected yet)'}</ProfileField>
                <ProfileField label="CPU cores">{profile.cpu_cores}</ProfileField>
                <ProfileField label="Memory">{profile.memory_gb} GB</ProfileField>
                <ProfileField label="CPU rating">{profile.cpu_score}</ProfileField>
                <ProfileField label="Graphics rating">{profile.gpu_score}</ProfileField>
                <ProfileField label="Graphics memory">{profile.gpu_memory_gb} GB</ProfileField>
                <ProfileField label="Graphics card" className="profile-field-wide">{profile.gpu_name || 'Unknown'}</ProfileField>
                <ProfileField label="Storage">{profile.storage_available_gb} / {profile.storage_total_gb} GB available</ProfileField>
                <ProfileField label="Location">{profile.location || 'local'}</ProfileField>
              </dl>
            ) : null}
            {!profileLoaded && !profileError && !profileLoading ? (
              <p className="text-sm text-muted-foreground">Computer details are not available yet.</p>
            ) : null}
          </CardContent>
        </Card>

        <Card>
          <CardHeader className="computer-card-heading">
            <span className="resource-icon" aria-hidden="true"><Wifi /></span>
            <div>
              <CardTitle className="text-base">Connection</CardTitle>
              <CardDescription>Worker network registration</CardDescription>
            </div>
          </CardHeader>
          <CardContent>
            {registration ? (
              <div className={`connection-state${registration.success ? '' : ' is-error'}`} role={registration.success ? 'status' : 'alert'}>
                {registration.success ? <CheckCircle2 aria-hidden="true" /> : null}
                <div>
                  <Badge variant={registration.success ? 'secondary' : 'destructive'}>
                    {registration.success ? 'Ready' : 'Not connected'}
                  </Badge>
                  <p>{registration.message}</p>
                  {registration.workerId ? (
                    <p className="connection-id"><span>Computer ID</span><strong>{registration.workerId}</strong></p>
                  ) : null}
                </div>
              </div>
            ) : (
              <div className="connection-state connection-empty">
                <Wifi aria-hidden="true" />
                <p>Sign in to connect this computer. When ready, it can receive assigned tasks.</p>
              </div>
            )}
          </CardContent>
        </Card>
      </div>
    </section>
  );
}
