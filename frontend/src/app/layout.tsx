import type { Metadata } from "next";
import "./globals.css";
import { ThemeProvider } from "@/components/site/theme-provider";
import { Toaster as SonnerToaster } from "@/components/ui/sonner";
import { DocumentLocale } from "@/components/site/document-locale";

export const metadata: Metadata = {
  title: "Hivemind 官方網站",
  description: "在共享的電腦網路上執行運算工作，或分享你的電腦。",
};

export default function RootLayout({
  children,
}: Readonly<{
  children: React.ReactNode;
}>) {
  return (
    <html lang="zh-Hant" suppressHydrationWarning>
      <body className="font-sans antialiased bg-background text-foreground min-h-screen">
        <ThemeProvider attribute="class" defaultTheme="light" enableSystem={false}>
          <DocumentLocale />
          {children}
          <SonnerToaster position="top-right" />
        </ThemeProvider>
      </body>
    </html>
  );
}
