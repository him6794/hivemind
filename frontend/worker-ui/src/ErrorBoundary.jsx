import React from 'react';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from '@/components/ui/card';

export class ErrorBoundary extends React.Component {
  constructor(props) {
    super(props);
    this.state = { hasError: false, error: null };
  }

  static getDerivedStateFromError(error) {
    return { hasError: true, error };
  }

  componentDidCatch(error, errorInfo) {
    console.error('ErrorBoundary caught:', error, errorInfo);
  }

  render() {
    if (this.state.hasError) {
      const onRetry = this.props.onRetry || (() => window.location.reload());
      return (
        <main className="load-error-screen">
          <Card className="load-error-card" role="alert">
            <CardHeader>
              <Badge variant="outline" className="w-fit">Hivemind Worker</Badge>
              <CardTitle>Worker console could not load</CardTitle>
              <CardDescription>Try again, or restart the Worker app if the problem continues.</CardDescription>
            </CardHeader>
            <CardContent>
              <Button type="button" onClick={onRetry}>Try again</Button>
            </CardContent>
          </Card>
        </main>
      );
    }
    return this.props.children;
  }
}
