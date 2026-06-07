from pydantic_settings import BaseSettings


class Settings(BaseSettings):
    database_url: str = "sqlite:///./dreamwell.db"
    data_dir: str = "./data"
    max_concurrent_jobs: int = 1
    sse_poll_interval: float = 0.25

    class Config:
        env_prefix = "DREAMWELL_"


settings = Settings()
