import seaborn as sns
import pandas as pd
import matplotlib.pyplot as plt

results = pd.read_csv("output/inference_times.csv")
plot = sns.lineplot(data=results, hue="Leafs", y="Runtime", x="Time", palette=sns.color_palette(), linewidth=0.5)
# plt.gca().set_xlim(numbers_of_splits)
# plt.gca().set_ylim([0, 120])
# plt.gca().set_xticks(range(9))
plt.gcf().tight_layout()
sns.despine(top=True, right=True)

plot.set_xlabel("Inference Step", fontsize=20)
plot.set_ylabel("Time / s", fontsize=20)
plot.tick_params(labelsize=15)
plot.legend(title="Leafs", fontsize=15)

plot.get_figure().savefig("output/time_plot.pdf", bbox_inches='tight')
# plot.get_figure().savefig("output/time_plot.png", bbox_inches='tight')