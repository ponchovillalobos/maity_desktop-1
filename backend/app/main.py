"""
Maity backend FastAPI app.

Resolves multiple findings from the assembly:
- PY-001: lifespan en lugar de @app.on_event("shutdown") (deprecado)
- PY-002: SummaryProcessor en lifespan startup en lugar de import-time
- PY-003: host/port/reload via env vars (MAITY_HOST, MAITY_PORT, MAITY_RELOAD)
- PY-004: logging centralizado con dictConfig
- PY-005: TranscriptProcessRequest BaseModel para validación
- SEC-001: CORS lista blanca con MAITY_CORS_ORIGINS env override
"""
import os
import logging
import logging.config
from contextlib import asynccontextmanager
from typing import Literal, Optional

from fastapi import FastAPI
from fastapi.middleware.cors import CORSMiddleware
import uvicorn
from dotenv import load_dotenv
from pydantic import BaseModel, Field, field_validator

from db import DatabaseManager
from transcript_processor import TranscriptProcessor

from routes import meetings_router, transcripts_router, summaries_router, config_router

# Load environment variables
load_dotenv()


# ============================================================================
# PY-004 — Centralized logging configuration (dictConfig)
# ============================================================================
LOG_LEVEL = os.getenv("MAITY_LOG_LEVEL", "INFO").upper()

LOGGING_CONFIG = {
    "version": 1,
    "disable_existing_loggers": False,
    "formatters": {
        "detailed": {
            "format": "%(asctime)s - %(name)s - %(levelname)s - "
                      "[%(filename)s:%(lineno)d - %(funcName)s()] - %(message)s",
            "datefmt": "%Y-%m-%d %H:%M:%S",
        },
    },
    "handlers": {
        "console": {
            "class": "logging.StreamHandler",
            "level": LOG_LEVEL,
            "formatter": "detailed",
            "stream": "ext://sys.stdout",
        },
    },
    "root": {
        "level": LOG_LEVEL,
        "handlers": ["console"],
    },
    "loggers": {
        "uvicorn": {"level": LOG_LEVEL},
        "uvicorn.access": {"level": LOG_LEVEL},
    },
}
logging.config.dictConfig(LOGGING_CONFIG)
logger = logging.getLogger(__name__)


# ============================================================================
# PY-005 — Pydantic request models
# ============================================================================
class TranscriptProcessRequest(BaseModel):
    """Request body for /process-transcript endpoint."""

    text: str = Field(..., min_length=1, description="Transcript text")
    model: Literal["openai", "anthropic", "groq", "ollama"] = Field(
        ..., description="LLM provider"
    )
    model_name: str = Field(..., min_length=1, description="Specific model identifier")
    chunk_size: int = Field(5000, gt=0, le=200000)
    overlap: int = Field(1000, ge=0)
    custom_prompt: str = Field(
        "Generate a summary of the meeting transcript.",
        max_length=4000,
    )

    @field_validator("overlap")
    @classmethod
    def overlap_lt_chunk_size(cls, v: int, info) -> int:
        chunk_size = info.data.get("chunk_size", 5000)
        if v >= chunk_size:
            return chunk_size - 1
        return v


# ============================================================================
# SummaryProcessor (PY-002 — instanciado en lifespan startup)
# ============================================================================
class SummaryProcessor:
    """Handles the processing of summaries in a thread-safe way"""

    def __init__(self):
        try:
            self.db = DatabaseManager()
            logger.info("Initializing SummaryProcessor components")
            self.transcript_processor = TranscriptProcessor()
            logger.info("SummaryProcessor initialized successfully (core components)")
        except Exception as e:
            logger.error(
                f"Failed to initialize SummaryProcessor: {str(e)}", exc_info=True
            )
            raise

    async def process_transcript(self, request: TranscriptProcessRequest) -> tuple:
        """Process a transcript text using a validated request object."""
        try:
            logger.info(
                f"Processing transcript of length {len(request.text)} with "
                f"chunk_size={request.chunk_size}, overlap={request.overlap}"
            )
            num_chunks, all_json_data = await self.transcript_processor.process_transcript(
                text=request.text,
                model=request.model,
                model_name=request.model_name,
                chunk_size=request.chunk_size,
                overlap=request.overlap,
                custom_prompt=request.custom_prompt,
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
            if hasattr(self, "transcript_processor"):
                self.transcript_processor.cleanup()
            logger.info("Cleanup completed successfully")
        except Exception as e:
            logger.error(f"Error during cleanup: {str(e)}", exc_info=True)


# Module-level handle to the processor (assigned in lifespan startup)
processor: Optional[SummaryProcessor] = None
db: Optional[DatabaseManager] = None


# ============================================================================
# PY-001 + PY-002 — Lifespan context manager (replaces @app.on_event)
# ============================================================================
@asynccontextmanager
async def lifespan(app: FastAPI):
    """Application lifespan: lazy init + graceful shutdown.

    PY-001: replaces deprecated @app.on_event("shutdown") in Starlette 0.27+.
    PY-002: SummaryProcessor instantiation moved here from import-time so the
            module imports cleanly even if dependencies fail (allows /health
            to respond with degraded mode).
    """
    global processor, db
    logger.info("Lifespan startup: initializing resources...")
    try:
        db = DatabaseManager()
        processor = SummaryProcessor()
        app.state.processor = processor
        app.state.db = db
        app.state.degraded = False
        logger.info("Lifespan startup completed successfully")
    except Exception as e:
        logger.error(
            f"Lifespan startup failed (degraded mode active): {e}", exc_info=True
        )
        app.state.processor = None
        app.state.db = None
        app.state.degraded = True

    yield

    # Shutdown
    logger.info("Lifespan shutdown: cleaning up resources...")
    try:
        if processor is not None:
            processor.cleanup()
        logger.info("Lifespan shutdown completed successfully")
    except Exception as e:
        logger.error(f"Error during shutdown cleanup: {str(e)}", exc_info=True)


# ============================================================================
# FastAPI app instantiation
# ============================================================================
app = FastAPI(
    title="Meeting Summarizer API",
    description="API for processing and summarizing meeting transcripts",
    version="2.0.0",
    lifespan=lifespan,
)

# Register custom error handler
from errors import AppError, app_error_handler  # noqa: E402

app.add_exception_handler(AppError, app_error_handler)


# ============================================================================
# SEC-001 — CORS restrictivo a orígenes Maity conocidos
# ============================================================================
_default_cors_origins = [
    "tauri://localhost",
    "https://tauri.localhost",
    "http://localhost:3118",
    "http://127.0.0.1:3118",
]
_extra_cors_origins = [
    o.strip()
    for o in os.getenv("MAITY_CORS_ORIGINS", "").split(",")
    if o.strip()
]
_allowed_cors_origins = _default_cors_origins + _extra_cors_origins

app.add_middleware(
    CORSMiddleware,
    allow_origins=_allowed_cors_origins,
    allow_credentials=True,
    allow_methods=["GET", "POST", "PUT", "DELETE", "OPTIONS"],
    allow_headers=["Authorization", "Content-Type", "X-Maity-Client-Version"],
    max_age=3600,
)
logger.info(f"CORS allowed origins: {_allowed_cors_origins}")


# Register routers
app.include_router(meetings_router)
app.include_router(transcripts_router)
app.include_router(summaries_router)
app.include_router(config_router)


# ============================================================================
# Health endpoint with degraded-mode awareness
# ============================================================================
@app.get("/health")
async def health():
    return {
        "status": "degraded" if getattr(app.state, "degraded", False) else "ok",
        "version": app.version,
    }


# ============================================================================
# PY-003 — uvicorn host/port/reload via env vars
# ============================================================================
if __name__ == "__main__":
    import multiprocessing

    multiprocessing.freeze_support()

    host = os.getenv("MAITY_HOST", "127.0.0.1")
    port = int(os.getenv("MAITY_PORT", "5167"))
    reload_enabled = os.getenv("MAITY_RELOAD", "0") == "1"

    if host == "0.0.0.0":
        logger.warning(
            "MAITY_HOST=0.0.0.0 — backend expuesto a la LAN. "
            "Asegurate de tener un firewall o bind a 127.0.0.1."
        )

    uvicorn.run("main:app", host=host, port=port, reload=reload_enabled)
