import express from "express";

function buildApp() {
  const app = express();
  app.get("/orders", (q, s) => s.json([]));
  return app;
}

export const appUnderTest = () => buildApp();
