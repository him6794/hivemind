import type { Metadata } from "next";
import "./globals.css";
import { ThemeProvider } from "@/components/site/theme-provider";
import { Toaster as SonnerToaster } from "@/components/ui/sonner";

export const metadata: Metadata = {
  title: "Hivemind | Official Site",
  description:
    "Hivemind explains distributed compute, manages account access and balance visibility, and guides users to deploy their own Master or Worker nodes.",
};

export default function RootLayout({
  children,
}: Readonly<{
  children: React.ReactNode;
}>) {
  return (
    <html lang="en" suppressHydrationWarning className="dark">
      <body className="font-sans font-mono antialiased bg-background text-foreground min-h-screen">
        <ThemeProvider attribute="class" defaultTheme="dark" enableSystem={false} disableTransitionOnChange>
          {children}
          <SonnerToaster position="top-right" />
        </ThemeProvider>
      </body>
    </html>
  );
}
