import type { Metadata } from "next";
import "./globals.css";
import { ThemeProvider } from "@/components/site/theme-provider";
import { Toaster as SonnerToaster } from "@/components/ui/sonner";

export const metadata: Metadata = {
  title: "Hivemind | Official Site",
  description:
    "Hivemind helps you run tasks on a shared network or share a computer with other users.",
};

export default function RootLayout({
  children,
}: Readonly<{
  children: React.ReactNode;
}>) {
  return (
    <html lang="en" suppressHydrationWarning>
      <body className="font-sans antialiased bg-background text-foreground min-h-screen">
        <ThemeProvider attribute="class" defaultTheme="light" enableSystem={false}>
          {children}
          <SonnerToaster position="top-right" />
        </ThemeProvider>
      </body>
    </html>
  );
}
