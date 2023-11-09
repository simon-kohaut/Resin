import seaborn as sns
import pandas as pd
import matplotlib.pyplot as plt

results = pd.read_csv("output/inference_times.csv")
results["Runtime"] = results["Runtime"].transform(lambda t: 1.0 / t) 
plot = sns.lineplot(data=results, hue="Location", y="Runtime", x="Time", palette=sns.color_palette(), linewidth=0.5)
plt.gcf().tight_layout()
sns.despine(top=True, right=True)

plot.set_xlabel("Inference Step", fontsize=20)
plot.set_ylabel("Frequency / Hz", fontsize=20)
plot.tick_params(labelsize=15)
plot.legend(title="Mean FoC", fontsize=15)

plot.get_figure().savefig("output/time_plot.pdf", bbox_inches='tight')

