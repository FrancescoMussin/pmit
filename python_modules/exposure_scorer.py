"""
Exposure scorer using sentence-BERT embeddings and cosine similarity.
Designed for fast batch scoring of trades with continuous [0,1] exposure scores.
"""

import sqlite3
# allows us to talk to rust via stdin and stdout
import sys
import numpy as np
# to make function signatures clearer
from typing import List, Tuple
# we borrow the model from here, and also helper functions
from sentence_transformers import SentenceTransformer, util


class ExposureScorer:
    def __init__(self, model_name: str = "all-MiniLM-L6-v2", temperature: float = 0.30):
        """
        Initialize the scorer with a sentence-BERT model.
        
        Args:
            model_name: HuggingFace model identifier (default is fast and accurate)
            temperature: Softmax temperature; lower values emphasize nearest matches
        """
        if temperature <= 0.0:
            raise ValueError("temperature must be > 0")

        # we load the sentence transformer model specified by model_name.
        self.model = SentenceTransformer(model_name)
        # we store the temperature for later use in weighting similarities.
        self.temperature = temperature
        # we initialize placeholders for training embeddings and scores, which will be loaded from the training database.
        self.training_embeddings = None
        self.training_scores = None
        self._load_training_data()

    def _softmax_weights(self, similarities: np.ndarray) -> np.ndarray:
        """Numerically stable, temperature-scaled softmax weights."""
        # divide similarities by temperature to control sharpness of weighting
        scaled = similarities / self.temperature
        # we can subtract the max from the scaled similarities to improve numerical stability when we take the exponential.
        # This prevents potential overflow issues when the similarities are large, which can lead to very large exponentials.
        # By subtracting the max, we ensure that the largest value in the scaled array is 0, which keeps the exponentials in 
        # a manageable range.
        scaled = scaled - np.max(scaled)
        # we take the exponential of the scaled similarities to get unnormalized weights.
        exp_scaled = np.exp(scaled)
        # we normalize the weights by dividing by their sum.
        return exp_scaled / np.sum(exp_scaled)
    
    def _load_training_data(self, training_db: str = "databases/training.db"):
        """Load and embed all training samples."""
        import os
        if not os.path.exists(training_db):
            print(f"[exposure_scorer] WARNING: Training database not found at {training_db}. All trades will receive 0.0 exposure.", file=sys.stderr)
            self.training_embeddings = None
            self.training_scores = np.array([], dtype=np.float32)
            return

        # open the connection to the training database.
        conn = sqlite3.connect(training_db)
        # cursor objects allow us to execute SQL queries and fetch results.
        cursor = conn.cursor()
        try:
            cursor.execute("SELECT title, outcome, exposure FROM training_samples")
            rows = cursor.fetchall()
        except sqlite3.OperationalError:
            print(f"[exposure_scorer] WARNING: training_samples table not found in {training_db}.", file=sys.stderr)
            rows = []
        finally:
            conn.close()
        
        if not rows:
            print(f"[exposure_scorer] WARNING: No training samples found. All trades will receive 0.0 exposure.", file=sys.stderr)
            self.training_embeddings = None
            self.training_scores = np.array([], dtype=np.float32)
            return
        
        # we unzip the rows into separate lists of titles, outcomes, and scores for easier processing.
        titles, outcomes, scores = zip(*rows)
        
        # Combine title and outcome for richer context
        texts = [f"On the market '{title}' I am betting '{outcome}'".strip() for title, outcome in zip(titles, outcomes)]
        
        # Embed all training samples
        self.training_embeddings = self.model.encode(texts, convert_to_tensor=True)
        # explicitly write down the type for consistency
        self.training_scores = np.array(scores, dtype=np.float32)
    
    def score_trade(self, title: str, outcome: str) -> float:
        """
        Score a single trade based on title and outcome.
        
        Uses weighted average of cosine similarities to training examples.
        High similarity to high-exposure trades → high score.
        
        Args:
            title: Market title
            outcome: Trade outcome
        
        Returns:
            Exposure score in [0, 1]
        """
        if not title or not outcome or self.training_embeddings is None:
            return 0.0
        
        text = f"On the market '{title}' I am betting '{outcome}'".strip()
        trade_embedding = self.model.encode(text, convert_to_tensor=True)
        
        # Cosine similarity to all training samples
        similarities = util.pytorch_cos_sim(trade_embedding, self.training_embeddings)[0]
        # numpy only works with tensors in cpu memory so we move it to cpu and convert to numpy array for the weighting step.
        similarities = similarities.cpu().numpy()
        
        # Weighted average: higher weight to more similar examples
        # Use temperature-scaled softmax to convert similarities to weights
        weights = self._softmax_weights(similarities)
        # compute the weighted sum
        score = np.dot(weights, self.training_scores)
        
        return float(score)
    
    def score_trades_batch(self, trades: List[Tuple[str, str]]) -> List[float]:
        """
        Score multiple trades efficiently.
        
        Args:
            trades: List of (title, outcome) tuples
        
        Returns:
            List of exposure scores in [0, 1]
        """
        if self.training_embeddings is None:
            return [0.0] * len(trades)

        texts = [f"On the market '{title}' I am betting '{outcome}'".strip() for title, outcome in trades]
        
        # Embed all at once (batched)
        trade_embeddings = self.model.encode(texts, convert_to_tensor=True)
        
        # Compute similarities for all trades
        similarities = util.pytorch_cos_sim(trade_embeddings, self.training_embeddings)
        
        # Weighted average for each trade
        scores = []
        for i in range(len(trades)):
            weights = self._softmax_weights(similarities[i].cpu().numpy())
            score = float(np.dot(weights, self.training_scores))
            scores.append(score)
        
        return scores