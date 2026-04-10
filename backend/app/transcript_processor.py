from pydantic import BaseModel
from typing import List, Tuple, Literal
from pydantic_ai import Agent
from pydantic_ai.models.anthropic import AnthropicModel
from pydantic_ai.models.groq import GroqModel
from pydantic_ai.models.openai import OpenAIModel
from pydantic_ai.providers.openai import OpenAIProvider
from pydantic_ai.providers.groq import GroqProvider
from pydantic_ai.providers.anthropic import AnthropicProvider

import logging
import os
from dotenv import load_dotenv
from db import DatabaseManager
from ollama import chat
import asyncio
from ollama import AsyncClient

# LLM-004: prompts localizados (es/en) — reemplaza el prompt hardcodeado en inglés
from prompts import build_prompt, detect_lang





# Set up logging
logging.basicConfig(
    level=logging.DEBUG,
    format='%(asctime)s - %(levelname)s - [%(filename)s:%(lineno)d] - %(message)s'
)
logger = logging.getLogger(__name__)

load_dotenv()  # Load environment variables from .env file

db = DatabaseManager()

class Block(BaseModel):
    """Represents a block of content in a section.
    
    Block types must align with frontend rendering capabilities:
    - 'text': Plain text content
    - 'bullet': Bulleted list item
    - 'heading1': Large section heading
    - 'heading2': Medium section heading
    
    Colors currently supported:
    - 'gray': Gray text color
    - '' or any other value: Default text color
    """
    id: str
    type: Literal['bullet', 'heading1', 'heading2', 'text']
    content: str
    color: str  # Frontend currently only uses 'gray' or default

class Section(BaseModel):
    """Represents a section in the meeting summary"""
    title: str
    blocks: List[Block]

class MeetingNotes(BaseModel):
    """Represents the meeting notes"""
    meeting_name: str
    sections: List[Section]

class People(BaseModel):
    """Represents the people in the meeting. Always have this part in the output. Title - Person Name (Role, Details)"""
    title: str
    blocks: List[Block]

class SummaryResponse(BaseModel):
    """Represents the meeting summary response based on a section of the transcript"""
    MeetingName : str
    People : People
    SessionSummary : Section
    CriticalDeadlines: Section
    KeyItemsDecisions: Section
    ImmediateActionItems: Section
    NextSteps: Section
    MeetingNotes: MeetingNotes

# --- Main Class Used by main.py ---

class TranscriptProcessor:
    """Handles the processing of meeting transcripts using AI models."""
    def __init__(self):
        """Initialize the transcript processor."""
        logger.info("TranscriptProcessor initialized.")
        self.db = DatabaseManager()
        self.active_clients = []  # Track active Ollama client sessions
    # LLM-001: Cap de tokens antes de invocar APIs de pago.
    # - MAITY_LLM_MAX_INPUT_TOKENS: cap total para toda la transcripción (default 500k).
    #   Previene que reuniones anómalas (ej. 12h loop de micrófono abierto) disparen
    #   costos no acotados en Claude/OpenAI/Groq.
    # - Estimación heurística rápida: 1 token ≈ 4 caracteres (tokenizer-agnostic,
    #   suficiente para abortar temprano; el backend LLM sigue siendo la fuente de verdad
    #   para el conteo real de billing).
    LLM_MAX_INPUT_TOKENS = int(os.getenv("MAITY_LLM_MAX_INPUT_TOKENS", "500000"))
    LLM_CHARS_PER_TOKEN = 4  # heurística estándar para prompts ES/EN mixtos

    @classmethod
    def estimate_tokens(cls, text: str) -> int:
        """LLM-001: Estima tokens de un texto con heurística char/4.

        No es exacto (cada proveedor tiene su tokenizer), pero es suficiente
        para un guard de seguridad previo a la invocación del LLM.
        """
        if not text:
            return 0
        return max(1, len(text) // cls.LLM_CHARS_PER_TOKEN)

    @classmethod
    def enforce_token_cap(cls, text: str, custom_prompt: str = "") -> int:
        """LLM-001: Valida que input_tokens <= LLM_MAX_INPUT_TOKENS.

        Levanta ValueError con mensaje accionable si se excede, ANTES de
        gastar un solo token en APIs de pago. Retorna el conteo estimado.
        """
        estimated = cls.estimate_tokens(text) + cls.estimate_tokens(custom_prompt)
        if estimated > cls.LLM_MAX_INPUT_TOKENS:
            raise ValueError(
                f"LLM-001: transcript excede el cap de tokens "
                f"({estimated} > {cls.LLM_MAX_INPUT_TOKENS}). "
                f"Ajusta MAITY_LLM_MAX_INPUT_TOKENS o segmenta la reunión antes de procesarla."
            )
        return estimated

    # LLM-006: Whitelist of recommended/supported model identifiers per provider.
    # Rejects deprecated or unknown model_name values to prevent silent failures
    # (e.g., user passing 'gpt-3.5-turbo' which is deprecated).
    MODEL_WHITELIST = {
        "openai": {
            "gpt-4o-mini",      # cost-effective default
            "gpt-4o",           # high-quality
            "o1-mini",
            "o1",
            "gpt-4-turbo",
        },
        "anthropic": {
            "claude-3-5-sonnet-latest",
            "claude-3-7-sonnet-latest",
            "claude-haiku-4-5-20251001",
            "claude-opus-4-6",
            "claude-3-5-haiku-latest",
        },
        "claude": {  # alias used in older configs
            "claude-3-5-sonnet-latest",
            "claude-3-7-sonnet-latest",
            "claude-haiku-4-5-20251001",
            "claude-opus-4-6",
            "claude-3-5-haiku-latest",
        },
        "groq": {
            "llama-3.3-70b-versatile",
            "llama-3.1-8b-instant",
            "mixtral-8x7b-32768",
        },
        # ollama is local and dynamic — no whitelist
    }

    @classmethod
    def validate_model(cls, provider: str, model_name: str) -> None:
        """LLM-006: Validate model_name against whitelist; raise ValueError if invalid."""
        if provider == "ollama":
            return  # ollama models are local and dynamic
        whitelist = cls.MODEL_WHITELIST.get(provider)
        if whitelist is None:
            return  # unknown provider — let the existing flow handle the error
        if model_name not in whitelist:
            valid = ", ".join(sorted(whitelist))
            raise ValueError(
                f"LLM-006: model_name '{model_name}' is not in the whitelist for "
                f"provider '{provider}'. Valid options: {valid}. "
                f"If you need a new model, update MODEL_WHITELIST."
            )

    async def process_transcript(self, text: str, model: str, model_name: str, chunk_size: int = 5000, overlap: int = 1000, custom_prompt: str = "") -> Tuple[int, List[str], List[dict]]:
        """
        Process transcript text into chunks and generate structured summaries for each chunk using an AI model.

        Args:
            text: The transcript text.
            model: The AI model provider ('claude', 'ollama', 'groq', 'openai').
            model_name: The specific model name.
            chunk_size: The size of each text chunk.
            overlap: The overlap between consecutive chunks.
            custom_prompt: A custom prompt to use for the AI model.

        Returns:
            A tuple containing:
            - The number of chunks processed.
            - A list of JSON strings, where each string is the summary of a chunk.
            - LLM-002: A list of dicts describing chunk errors (empty if all succeeded).
              Each dict: {"chunk": int, "error": str}
        """

        logger.info(f"Processing transcript (length {len(text)}) with model provider={model}, model_name={model_name}, chunk_size={chunk_size}, overlap={overlap}")
        # LLM-006: Validate model BEFORE invoking any provider
        self.validate_model(model, model_name)
        # LLM-001: Cap de tokens ANTES de invocar APIs de pago.
        estimated_tokens = self.enforce_token_cap(text, custom_prompt)
        logger.info(f"LLM-001: estimated_input_tokens={estimated_tokens} (cap={self.LLM_MAX_INPUT_TOKENS})")

        all_json_data = []
        chunk_errors: List[dict] = []  # LLM-002: track per-chunk failures
        agent = None # Define agent variable
        llm = None # Define llm variable

        try:
            # Select and initialize the AI model and agent
            if model == "claude":
                api_key = await db.get_api_key("claude")
                if not api_key: raise ValueError("ANTHROPIC_API_KEY environment variable not set")
                llm = AnthropicModel(model_name, provider=AnthropicProvider(api_key=api_key))
                logger.info(f"Using Claude model: {model_name}")
            elif model == "ollama":
                # Use environment variable for Ollama host configuration
                ollama_host = os.getenv('OLLAMA_HOST', 'http://localhost:11434')
                ollama_base_url = f"{ollama_host}/v1"
                ollama_model = OpenAIModel(
                    model_name=model_name, provider=OpenAIProvider(base_url=ollama_base_url)
                )
                llm = ollama_model
                if model_name.lower().startswith("phi4") or model_name.lower().startswith("llama"):
                    chunk_size = 10000
                    overlap = 1000
                else:
                    chunk_size = 30000
                    overlap = 1000
                logger.info(f"Using Ollama model: {model_name}")
            elif model == "groq":
                api_key = await db.get_api_key("groq")
                if not api_key: raise ValueError("GROQ_API_KEY environment variable not set")
                llm = GroqModel(model_name, provider=GroqProvider(api_key=api_key))
                logger.info(f"Using Groq model: {model_name}")
            # --- ADD OPENAI SUPPORT HERE ---
            elif model == "openai":
                api_key = await db.get_api_key("openai")
                if not api_key: raise ValueError("OPENAI_API_KEY environment variable not set")
                llm = OpenAIModel(model_name, provider=OpenAIProvider(api_key=api_key))
                logger.info(f"Using OpenAI model: {model_name}")
            # --- END OPENAI SUPPORT ---
            else:
                logger.error(f"Unsupported model provider requested: {model}")
                raise ValueError(f"Unsupported model provider: {model}")

            # LLM-007: Bump result_retries 2 → 5. Para modelos pequeños (haiku,
            # llama-3.1-8b) y schemas anidados, 2 reintentos no bastan; 5 da
            # margen para corregir tras parse-fail manteniendo cap razonable.
            agent = Agent(
                llm,
                result_type=SummaryResponse,
                result_retries=5,
            )
            logger.info("Pydantic-AI Agent initialized.")

            # Split transcript into chunks
            step = chunk_size - overlap
            if step <= 0:
                logger.warning(f"Overlap ({overlap}) >= chunk_size ({chunk_size}). Adjusting overlap.")
                overlap = max(0, chunk_size - 100)
                step = chunk_size - overlap

            chunks = [text[i:i+chunk_size] for i in range(0, len(text), step)]
            num_chunks = len(chunks)
            logger.info(f"Split transcript into {num_chunks} chunks.")

            # LLM-004: detectar idioma del transcript para usar prompt localizado (es/en)
            full_text_sample = " ".join(chunks[:3])
            lang = detect_lang(full_text_sample)
            logger.info(f"LLM-004: detected language '{lang}' for transcript (chunks={num_chunks})")

            for i, chunk in enumerate(chunks):
                logger.info(f"Processing chunk {i+1}/{num_chunks}...")
                try:
                    # Run the agent to get the structured summary for the chunk
                    if model != "ollama":
                        localized_prompt = build_prompt(lang, chunk, custom_prompt)
                        summary_result = await agent.run(localized_prompt)
                    else:
                        logger.info(f"Using Ollama model: {model_name} and chunk size: {chunk_size} with overlap: {overlap}")
                        response = await self.chat_ollama_model(model_name, chunk, custom_prompt)
                        
                        # Check if response is already a SummaryResponse object or a string that needs validation
                        if isinstance(response, SummaryResponse):
                            summary_result = response
                        else:
                            # If it's a string (JSON), validate it
                            summary_result = SummaryResponse.model_validate_json(response)
                            
                        logger.info(f"Summary result for chunk {i+1}: {summary_result}")
                        logger.info(f"Summary result type for chunk {i+1}: {type(summary_result)}")

                    if hasattr(summary_result, 'data') and isinstance(summary_result.data, SummaryResponse):
                         final_summary_pydantic = summary_result.data
                    elif isinstance(summary_result, SummaryResponse):
                         final_summary_pydantic = summary_result
                    else:
                         logger.error(f"Unexpected result type from agent for chunk {i+1}: {type(summary_result)}")
                         continue # Skip this chunk

                    # Convert the Pydantic model to a JSON string
                    chunk_summary_json = final_summary_pydantic.model_dump_json()
                    all_json_data.append(chunk_summary_json)
                    logger.info(f"Successfully generated summary for chunk {i+1}.")

                except Exception as chunk_error:
                    err_msg = str(chunk_error)
                    logger.error(f"Error processing chunk {i+1}: {err_msg}", exc_info=True)
                    # LLM-002: registrar el fallo para surfacear al cliente
                    chunk_errors.append({"chunk": i + 1, "error": err_msg})

            if chunk_errors:
                logger.warning(
                    f"LLM-002: {len(chunk_errors)}/{num_chunks} chunks failed — "
                    f"summary is PARTIAL. Failed: {[e['chunk'] for e in chunk_errors]}"
                )
            logger.info(f"Finished processing all {num_chunks} chunks.")
            return num_chunks, all_json_data, chunk_errors

        except Exception as e:
            logger.error(f"Error during transcript processing: {str(e)}", exc_info=True)
            raise
    
    async def chat_ollama_model(self, model_name: str, transcript: str, custom_prompt: str):
        # LLM-004: usar prompt localizado (es/en) para Ollama también
        lang = detect_lang(transcript)
        localized_content = build_prompt(lang, transcript, custom_prompt)
        message = {
            'role': 'system',
            'content': localized_content,
        }

        # Create a client and track it for cleanup
        ollama_host = os.getenv('OLLAMA_HOST', 'http://127.0.0.1:11434')
        client = AsyncClient(host=ollama_host)
        self.active_clients.append(client)
        
        try:
            response = await client.chat(model=model_name, messages=[message], stream=True, format=SummaryResponse.model_json_schema())
            
            full_response = ""
            async for part in response:
                content = part['message']['content']
                print(content, end='', flush=True)
                full_response += content
            
            try:
                summary = SummaryResponse.model_validate_json(full_response)
                print("\n", summary.model_dump_json(indent=2), type(summary))
                return summary
            except Exception as e:
                print(f"\nError parsing response: {e}")
                return full_response
        except asyncio.CancelledError:
            logger.info("Ollama request was cancelled during shutdown")
            raise
        except Exception as e:
            logger.error(f"Error in Ollama chat: {e}")
            raise
        finally:
            # Remove the client from active clients list
            if client in self.active_clients:
                self.active_clients.remove(client)

    def cleanup(self):
        """Clean up resources used by the TranscriptProcessor."""
        logger.info("Cleaning up TranscriptProcessor resources")
        try:
            # Close database connections if any
            if hasattr(self, 'db') and self.db is not None:
                # self.db.close()
                logger.info("Database connection cleanup (using context managers)")
                
            # Cancel any active Ollama client sessions
            if hasattr(self, 'active_clients') and self.active_clients:
                logger.info(f"Terminating {len(self.active_clients)} active Ollama client sessions")
                for client in self.active_clients:
                    try:
                        # Close the client's underlying connection
                        if hasattr(client, '_client') and hasattr(client._client, 'close'):
                            asyncio.create_task(client._client.aclose())
                    except Exception as client_error:
                        logger.error(f"Error closing Ollama client: {client_error}", exc_info=True)
                # Clear the list
                self.active_clients.clear()
                logger.info("All Ollama client sessions terminated")
        except Exception as e:
            logger.error(f"Error during TranscriptProcessor cleanup: {str(e)}", exc_info=True)

        