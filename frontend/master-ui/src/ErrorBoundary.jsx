import React from 'react';

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
        <div style={{
          minHeight: '100vh',
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'center',
          fontFamily: 'Inter, system-ui, sans-serif',
          background: 'linear-gradient(180deg, #f4f7fb 0%, #fafcff 100%)',
          padding: 32,
        }}>
          <div style={{
            maxWidth: 480,
            textAlign: 'center',
            border: '1px solid #d8e0e8',
            borderRadius: 14,
            background: '#fff',
            padding: 32,
            boxShadow: '0 12px 32px rgba(15, 23, 42, 0.06)',
          }}>
            <h2 style={{ margin: '0 0 12px', fontSize: 22, color: '#c62828' }}>
              Something went wrong
            </h2>
            <p style={{ margin: '0 0 20px', color: '#5e6c7a', fontSize: 14 }}>
              {this.state.error?.message || 'An unexpected error occurred.'}
            </p>
            <button
              type="button"
              onClick={onRetry}
              style={{
                padding: '10px 20px',
                border: 'none',
                borderRadius: 10,
                cursor: 'pointer',
                fontWeight: 700,
                background: '#1769aa',
                color: '#fff',
              }}
            >
              Try again
            </button>
          </div>
        </div>
      );
    }
    return this.props.children;
  }
}
