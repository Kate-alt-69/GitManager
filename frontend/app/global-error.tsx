"use client";

import "./globals.css";

export default function GlobalErrorBoundary({
  error,
  reset,
}: {
  error: Error & { digest?: string };
  reset: () => void;
}) {
  return (
    <html lang="en">
      <body>
        <main className="gm-error-screen">
          <section className="gm-error-content" role="alert">
            <h1>GitManager could not start this view</h1>
            <p>{error.message || "An unexpected application error occurred."}</p>
            <button type="button" onClick={reset}>Try again</button>
          </section>
        </main>
      </body>
    </html>
  );
}
