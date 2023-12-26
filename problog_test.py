from problog.program import PrologString
from problog import get_evaluatable
import seaborn as sns
import pandas as pd
import matplotlib.pyplot as plt
from tqdm import tqdm
from scipy.stats import norm
from time import time

data = pd.read_csv("pairwise_closeness.csv")

for timestamp in data["t"].unique():
    model = ""
    simultaneous = data[data["t"] == timestamp]

    for _, row in simultaneous.iterrows():
        model += f'{row["p_close"]}::close({row["d1"]}, {row["d2"]}).\n'

    model += "unsafe :- close(X, Y).\n"
    model += "query(unsafe)."
    
    print(model)

    executor = get_evaluatable().create_from(PrologString(model))

    start = time()
    result = executor.evaluate()
    elapsed = time() - start

    print(result, elapsed)
