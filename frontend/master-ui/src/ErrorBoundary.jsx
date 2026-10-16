import React from 'react';
import { AlertTriangle } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
import './console.css';
import { LocaleContext, LocaleSelect } from './LocaleProvider.jsx';

export class ErrorBoundary extends React.Component {
  static contextType = LocaleContext;
  constructor(props) {
    super(props);
    this.state = { hasError: false, error: null };
  }

  static getDerivedStateFromError(error) {
    return { hasError: true, error };
  }

  componentDidMount() {
    document.getElementById('startup-fallback')?.remove();
  }

  componentDidCatch(error, errorInfo) {
    console.error('ErrorBoundary caught:', error, errorInfo);
  }

  render() {
    const { t } = this.context;
    if (this.state.hasError) {
      const onRetry = this.props.onRetry || (() => window.location.reload());
      return (
        <main className="error-screen">
          <Card className="error-card">
            <CardHeader className="error-card-header">
              <span className="error-icon" aria-hidden="true"><AlertTriangle /></span>
              <LocaleSelect />
              <CardTitle className="section-title" role="heading" aria-level="1">{t('Something went wrong')}</CardTitle>
              <CardDescription>{t('Hivemind could not load this page. Try again, or return later.')}</CardDescription>
            </CardHeader>
            <CardContent className="error-card-content">
              <Button type="button" onClick={onRetry}>{t("Try again")}</Button>
            </CardContent>
          </Card>
        </main>
      );
    }
    return this.props.children;
  }
}
