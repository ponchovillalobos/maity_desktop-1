import os

from fastapi import FastAPI
from fastapi.middleware.cors import CORSMiddleware
import uvicorn
import logging
from dotenv import load_dotenv
from db import DatabaseManager
from transcript_processor import TranscriptProcessor

from routes import meetings_router, transcripts_router, summaries_router, config_router

# Load environment variables
load_dotenv()

# Configure logger with line numbers and function names
logger = logging.getLogger(__name__)
logger.setLevel(logging.DEBUG)

# Create console handler with formatting
console_handler = logging.StreamHandler()
console_handler.setLevel(logging.DEBUG)

# Create formatter with line numbers and function names
formatter = logging.Formatter(
    '%(asctime)s - %(levelname)s - [%(filename)s:%(lineno)d - %(funcName)s()] - %(message)s',
    datefmt='%Y-%m-%d %H:%M:%S'
)
console_handler.setFormatter(formatter)

# Add handler to logger if not already added
if not logger.handlers:
    logger.addHandler(console_handler)

app = FastAPI(
    title="Meeting Summarizer API",
    description="API for processing and summarizing meeting transcripts",
    version="1.0.0"
)

# Register custom error handler
from errors import AppError, app_error_handler
app.add_exception_handler(AppError, app_error_handler)

# Configure CORS (SEC-001 — restricted to known Maity origins).
# Defaults to ONLY the Tauri app + Next.js dev server.
# Override with MAITY_CORS_ORIGINS env var (comma-separated) for custom deployments.
_default_origins = [
    "tauri://localhost",          # Tauri webview (production)
    "https://tauri.localhost",    # Tauri webview (Windows alternate scheme)
    "http://localhost:3118",      # Next.js dev server (frontend/package.json)
    "http://127.0.0.1:3118",
]
_extra_origins = [
    o.strip()
    for o in os.getenv("MAITY_CORS_ORIGINS", "").split(",")
    if o.strip()
]
_allowed_origins = _default_origins + _extra_origins

# Note: with allow_credentials=True, wildcard "*" is rejected by spec.
# Listing explicit origins is the only valid path for credentialed requests.
app.add_middleware(
    CORSMiddleware,
    allow_origins=_allowed_origins,
    allow_credentials=True,
    allow_methods=["GET", "POST", "PUT", "DELETE", "OPTIONS"],
    allow_headers=["Authorization", "Content-Type", "X-Maity-Client-Version"],
    max_age=3600,
)
logger.info(f"CORS allowed origins: {_allowed_origins}")

# Global database manager instance for meeting management endpoints
db = DatabaseManager()


class SummaryProcessor:
    """Handles the processing of summaries in a thread-safe way"""
    def __init__(self):
        try:
            self.db = DatabaseManager()

            logger.info("Initializing SummaryProcessor components")
            self.transcript_processor = TranscriptProcessor()
            logger.info("SummaryProcessor initialized successfully (core components)")
        except Exception as e:
            logger.error(f"Failed to initialize SummaryProcessor: {str(e)}", exc_info=True)
            raise

    async def process_transcript(self, text: str, model: str, model_name: str, chunk_size: int = 5000, overlap: int = 1000, custom_prompt: str = "Generate a summary of the meeting transcript.") -> tuple:
        """Process a transcript text"""
        try:
            if not text:
                raise ValueError("Empty transcript text provided")

            if chunk_size <= 0:
                raise ValueError("chunk_size must be positive")
            if overlap < 0:
                raise ValueError("overlap must be non-negative")
            if overlap >= chunk_size:
                overlap = chunk_size - 1

            step_size = chunk_size - overlap
            if step_size <= 0:
                chunk_size = overlap + 1

            logger.info(f"Processing transcript of length {len(text)} with chunk_size={chunk_size}, overlap={overlap}")
            num_chunks, all_json_data = await self.transcript_processor.process_transcript(
                text=text,
                model=model,
                model_name=model_name,
                chunk_size=chunk_size,
                overlap=overlap,
                custom_prompt=custom_prompt
            )
            logger.info(f"Successfully processed transcript into {num_chunks} chunks")

            return num_chunks, all_json_data
        except Exception as e:
            logger.error(f"Error processing transcript: {str(e)}", exc_info=True)
            raise

    def cleanup(self):
        """Cleanup resources"""
        try:
            logger.info("Cleaning up resources")
            if hasattr(self, 'transcript_processor'):
                self.transcript_processor.cleanup()
            logger.info("Cleanup completed successfully")
        except Exception as e:
            logger.error(f"Error during cleanup: {str(e)}", exc_info=True)


# Initialize processor
processor = SummaryProcessor()

# Register routers
app.include_router(meetings_router)
app.include_router(transcripts_router)
app.include_router(summaries_router)
app.include_router(config_router)


@app.on_event("shutdown")
async def shutdown_event():
    """Cleanup on API shutdown"""
    logger.info("API shutting down, cleaning up resources")
    try:
        processor.cleanup()
        logger.info("Successfully cleaned up resources")
    except Exception as e:
        logger.error(f"Error during cleanup: {str(e)}", exc_info=True)

if __name__ == "__main__":
    import multiprocessing
    multiprocessing.freeze_support()

    # PY-003: leer host/port/reload de variables de entorno con defaults seguros.
    # Defaults: bind a 127.0.0.1 (no exponer LAN), reload deshabilitado (solo dev opt-in).
    # Override con env vars: MAITY_HOST, MAITY_PORT, MAITY_RELOAD=1.
    host = os.getenv("MAITY_HOST", "127.0.0.1")
    port = int(os.getenv("MAITY_PORT", "5167"))
    reload_enabled = os.getenv("MAITY_RELOAD", "0") == "1"

    if host == "0.0.0.0":
        logger.warning(
            "MAITY_HOST=0.0.0.0 — backend expuesto a la LAN. "
            "Asegurate de tener un firewall o bind a 127.0.0.1."
        )

    uvicorn.run("main:app", host=host, port=port, reload=reload_enabled)
