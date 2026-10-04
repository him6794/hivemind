import { Check, Circle, LoaderCircle, TriangleAlert } from 'lucide-react';
import { Button } from '@/components/ui/button';
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from '@/components/ui/card';
import { Skeleton } from '@/components/ui/skeleton';

const STARTUP_STEPS = [
  { phase: 'starting', label: 'Start Worker' },
  { phase: 'connecting', label: 'Connect services' },
  { phase: 'resources', label: 'Check this computer' },
  { phase: 'services', label: 'Prepare worker' },
];

const STARTUP_PHASE_ORDER = Object.fromEntries(STARTUP_STEPS.map(({ phase }, index) => [phase, index]));

function startupCopy(phase, flow) {
  if (flow === 'native' && phase === 'connecting') {
    return ['Preparing network', 'Preparing local Worker network services.'];
  }
  if (flow === 'native' && phase === 'services') {
    return ['Starting local services', 'Starting the local services required by Worker.'];
  }
  if (phase === 'connecting') return ['Signing in and connecting', 'Connecting to Hivemind and preparing a secure worker session.'];
  if (phase === 'resources') return ['Checking this computer', 'Reading the local worker profile and available resources.'];
  if (phase === 'services') return ['Preparing this computer', 'Registering this worker with Hivemind so it can receive tasks.'];
  if (phase === 'ready') return ['Worker service is ready', 'The local Worker service is ready for setup.'];
  return ['Starting Worker', 'Waiting for the local Worker service to become ready.'];
}

export function LoadingPanel({
  phase = 'starting',
  error = '',
  onRetry,
  retryLabel = 'Try again',
  showSkeleton = false,
  compact = false,
  flow = compact ? 'onboarding' : 'native',
}) {
  const [title, description] = error
    ? ['Worker setup needs attention', 'Check the message below, then retry when the service is available.']
    : startupCopy(phase, flow);
  const activeIndex = Math.max(0, STARTUP_PHASE_ORDER[phase] ?? 0);

  return (
    <Card className={`startup-card${compact ? ' is-compact' : ''}`} aria-live="polite" aria-busy={!error}>
      <CardHeader className="startup-card-heading">
        <div className={`startup-icon${error ? ' is-error' : ''}`} aria-hidden="true">
          {error ? <TriangleAlert /> : <LoaderCircle className="animate-spin motion-reduce:animate-none" />}
        </div>
        <div>
          <p className="section-kicker">Hivemind Worker</p>
          <CardTitle className="startup-title">{title}</CardTitle>
          <CardDescription>{description}</CardDescription>
        </div>
      </CardHeader>
      <CardContent>
        {showSkeleton ? (
          <div className="startup-skeleton" aria-hidden="true">
            <Skeleton /><Skeleton /><Skeleton />
          </div>
        ) : null}
        <ol className="startup-steps" aria-label="Worker setup progress">
          {STARTUP_STEPS.map(({ phase: stepPhase, label }, index) => {
            const stepLabel = flow === 'native'
              ? stepPhase === 'connecting' ? 'Prepare network' : stepPhase === 'services' ? 'Start local services' : label
              : stepPhase === 'services' ? 'Register worker' : label;
            const completed = index < activeIndex;
            const current = index === activeIndex && !error;
            return (
              <li
                key={stepPhase}
                className={completed ? 'is-complete' : current ? 'is-current' : ''}
                aria-current={current ? 'step' : undefined}
              >
                <span className="startup-step-icon" aria-hidden="true">
                  {completed ? <Check /> : current ? <Circle className="startup-current-mark" /> : <span>{index + 1}</span>}
                </span>
                <span>{stepLabel}</span>
              </li>
            );
          })}
        </ol>
        {error ? (
          <div className="startup-error" role="alert">
            <p>{error}</p>
            {onRetry ? <Button type="button" variant="outline" onClick={onRetry}>{retryLabel}</Button> : null}
          </div>
        ) : null}
      </CardContent>
    </Card>
  );
}
