import seaborn as sns
import pandas as pd
import matplotlib.pyplot as plt

results = pd.read_csv("output/data/frequencies.csv")
plot = sns.displot(data=results, hue="Location", x="Frequency", clip=(0, 1000), common_norm=False, kind="kde", palette=sns.color_palette())
plt.gcf().tight_layout()
sns.despine(top=True, right=True)

plot.set_xlabels("Frequency / Hz", fontsize=20)
plot.set_ylabels("Density", fontsize=20)
plot.tick_params(labelsize=15)
# plot.legends(title="Shape", fontsize=15)

plot.savefig("output/plots/frequencies_plot.pdf", bbox_inches='tight')
