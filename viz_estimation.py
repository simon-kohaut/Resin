import seaborn as sns
import pandas as pd
import matplotlib.pyplot as plt

results = pd.read_csv("output/data/foc_estimation.csv")
plot = sns.lineplot(data=results, x="Measurement", y="Error", label="FoC Estimate");
plot = sns.lineplot(data=results, x="Measurement", y="ClusterError", label="FoC Cluster");
plot.set_aspect(10.0 / 1.0)
plt.gcf().tight_layout()
sns.despine(top=True, right=True)

plot.set_xlabel("#Measurements", fontsize=20)
plot.set_ylabel("Asbolute Error", fontsize=20)
plot.tick_params(labelsize=15)
plot.set_ylim([0, 5])
plot.set_yticks([0, 2.5, 5])

plot.get_figure().savefig("output/plots/foc_estimation.pdf", bbox_inches='tight')
