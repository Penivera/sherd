// Base URL of the sherd-auth FastAPI service. Set in .env (see .env.example).
// Never hardcode a production URL here.
export const API_BASE_URL = import.meta.env.VITE_API_BASE_URL || "http://localhost:8000";
