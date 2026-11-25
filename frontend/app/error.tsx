"use client";

export default function ErrorBoundary({
  error,
  reset,
}: {
  error: Error & { digest?: string };
  reset: () => void;
}) {
  return (
    <main className="gm-error-screen">
      <section className="gm-error-content" role="alert">
        <h1>GitManager ran into a problem</h1>
        <p>{error.message || "The current view could not be rendered."}</p>
        <button type="button" onClick={reset}>Try again</button>
      </section>
    </main>
  );
}
