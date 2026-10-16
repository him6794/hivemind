import React from 'react';
import { LocaleContext, LocaleSelect } from './LocaleProvider.jsx';
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
  static contextType = LocaleContext;
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
    const { t } = this.context;
    if (this.state.hasError) {
      const onRetry = this.props.onRetry || (() => window.location.reload());
      return (
        <main className="load-error-screen">
          <Card className="load-error-card" role="alert">
            <CardHeader>
              <LocaleSelect />
              <Badge variant="outline" className="w-fit">Hivemind Worker</Badge>
              <CardTitle>{t("Worker console could not load")}</CardTitle>
              <CardDescription>{t("Try again, or restart the Worker app if the problem continues.")}</CardDescription>
            </CardHeader>
            <CardContent>
              <Button type="button" onClick={onRetry}>{t("Try again")}</Button>
            </CardContent>
          </Card>
        </main>
      );
    }
    return this.props.children;
  }
}
